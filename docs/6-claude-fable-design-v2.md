# Design Document: webcam-handler — Architecture (v2)

Doc 6 in the webcam-handler series, **v2 — first revision**. Status: current; supersedes
docs/1 (v1, now under `docs/historical/`), whose section and registry numbering this
revision preserves, so v1 citations in the implementation notes and the commit history
stay resolvable. Companion documents: the phased implementation plan (docs/7), the code
review rubric (docs/8), the automated quality gates (docs/9), and AGENTS.md (docs/10,
deployed at the repository root).

**What this document was built from.** Four inputs, in decreasing order of authority:

1. **Hardware probes** — the design-phase probes (2026-08-07, kernel 7.0.0-29-generic,
   rustc 1.97.1) against the attached cameras (a Chicony integrated camera, 04f2:b83c,
   which is *two* logical cameras: RGB + IR; and an OBSBOT Tiny 3, a PTZ camera), plus
   the P1/P2 implementation-phase measurements (2026-08-08, same host and hardware). The
   probe record is §1.2; findings are cited as **[PF:n]** throughout. Where a probe
   finding contradicts a crate's documentation or the V4L2 specification, the probe wins
   (E1, §2.12).
2. **Implementation case law** — `docs/implementation-notes.md`: the recorded, justified
   deviations (N-entries) and dated phase evidence (E-entries) from building P0–P2
   against docs/1–5. This revision absorbs the entries that said "the design should
   absorb this at its next revision" — N2, N4, N5, N6, N7, N9, and N8 as §2.13 — with
   the notes remaining the primary measurement record. Absorption does not retire an
   entry; it moves a law's home from case law into the design.
3. **The v4l2-webcam agent skill** (`vendor/v4l2-webcam-skill/`), which records the manual
   command sequences this tool exists to replace. Every operation the skill teaches an agent
   to perform with `v4l2-ctl`/`ffmpeg` maps to a first-class operation here (§1.1).
4. **Crate research with adversarial license audits** (2026-08-07, crates.io + repository
   verification). Versions and licenses in §2.8 are as verified on that date, minus one
   drop the license gate itself forced (`directories`, note N2).

## Changes from v1

Every change absorbs measured evidence; none re-litigates a settled decision. Each row
names its source, so a reviewer can check the absorption against the record:

| Change | Where | Source |
|---|---|---|
| The error registry grows from fourteen variants to nineteen: `CameraUnknown`, `CameraAmbiguous`, `DeviceIo`, `StorageIo` complete it; `Unimplemented` is transitional and scheduled to die at P4 | D13 | N4, N6 |
| `CameraBackend` gains `diagnose`, with a default empty body; the T1/T2 sketches now show the shipped shapes (`kind` required, `name` derived) | T1 (§2.3) | N7 |
| Restore's outcome vocabulary gains a fourth outcome, `OwnedByAutomation`, and it counts as complete | D4 | N9 |
| Grouping and fingerprints come from the sysfs USB *interface* path; `bus_info` cannot tell two logical cameras apart and is never a USB grouping or fingerprint key | D1, §1.2 | PF:13 |
| The driverless-camera diagnosis is asked per USB device, not per interface, and reaches clients through `diagnose` | D1 | PF:14, N7 |
| `ENOTTY` joins `EINVAL` as an enumeration terminator; from `QUERYCAP` it still means "not a V4L2 device" | §2.5, §1.2 | PF:15 |
| EXIF stamping is a header-only APP1 splice; nothing parses camera bytes past `SOS` | D6 | PF:16 |
| Empirical pair discovery treats an automation menu as a menu, not a switch: every alternative tried, residue isolated, "off" recorded per freed control | D3 | E4 (notes) |
| Stepwise frame-size entries answer "closest deliverable size" from the whole range, never collapsed to a corner | D5 | E4 (notes) |
| The write path's ioctl dispatch belongs to the *descriptor*, never to the caller's value variant | T2 (§2.3) | E4 (notes) |
| `webcam-handler-api` is exempt from the tokio half of the T6 wall; its own wall (no axum/hyper/tower-http) is gate-asserted | §2.8 | N5 |
| `directories` is not a dependency; the two XDG paths are ~thirty owned lines (`option-ext` is MPL-2.0) | §2.8 | N2 |
| The dev-only privileged helper `wch-priv`, its boundary, and its G6 revisit trigger are recorded | §2.13 (new) | N8 |
| The R2 rung has executed, gained write/stream arms, and serializes with R3; §3.1 restates what its green proves | §3.1 | E2, E3 (notes) |
| The structural-gap register is regenerated: single-host evidence and the mid-stream device-loss limit join it | §3.3 | E3 (notes), N8 |
| The PF registry runs PF:1–16; PF:13–16 were measured during implementation | §1.2 | notes |
| Testing exercises every control by default, motors included; `WCH_NO_MOTION=1` is the opt-out, and the product's `--allow-motion` posture is unchanged | §5 | owner ruling, 2026-08-08 |

## TL;DR

- **Architecture:** one schema crate whose types serve four masters — the library API, the
  JSON-RPC wire, `--json` CLI output, and on-disk session state; a pure engine that takes a
  camera backend as a value behind one trait (T1/T2); a V4L2 backend as the first vertical,
  with the fake backend replaying **captured device profiles** from real hardware; and a
  calibration engine whose sessions are human-inspectable JSON directories.
- **The device is the only authority on itself (E2).** Capabilities, controls, menus, and
  ranges are read from the device at open time, never cached across sessions as truth.
  Probing two real cameras surfaced sparse menus [PF:2], values outside declared ranges
  [PF:4], defaults outside declared ranges [PF:5], and silently clamped writes [PF:6] —
  the control model represents all of these instead of "correcting" them.
- **Every write reads back (D3, E4).** V4L2 drivers clamp out-of-range writes and report
  success [PF:6]; a set operation's result is always `{requested, applied}`.
- **Represent the unknown; never panic on it (D2).** The most popular V4L2 crate panics on
  a control type introduced by newer kernels [PF:1]; we own the control-enumeration layer
  with raw ioctls, and unknown types/flags are carried as data.
- **In-process everything (§1 constraint).** No `v4l2-ctl`, no `ffmpeg`, no external
  binaries at runtime. Photos are camera-native JPEG bytes passed through verbatim or
  PNG-encoded in Rust; video is MJPEG remuxed into AVI by our own ~300-line muxer (no
  encoder, no codec patents, no copyleft). Build-time deps (bindgen/libclang) are accepted.
- **Daemon security by construction (D11):** JSON-RPC over a Unix socket in
  `$XDG_RUNTIME_DIR` (mode-0700 directory) is always on; TCP for the web client is opt-in,
  loopback-bound, token-gated.
- **Licenses:** permissive only, enforced by cargo-deny with named bans for the known traps
  in this domain — LGPL `libv4l`/`libudev`/`libcamera`/`alsa-lib` linkage, MPL `colored`
  and `minimp4`, GPL codec wrappers (§2.8). The one IJG-license question in this domain is
  audited **dormant, not open**: the default stack — including `image`'s own MIT/Apache
  JPEG encoder — is allowlist-clean, and the IJG-carrying crates sit on the ban list so the
  license can only arrive by decision, not by accident (§8 item 1).

## 1. Scope

**Goals (v1).** A Rust 2024 edition Cargo workspace providing, for V4L2 webcams on Linux,
behind a pluggable backend trait:

- **Enumerate** connected cameras with stable identities, correctly grouping multi-node and
  multi-camera USB devices [PF:7] and distinguishing capture from metadata nodes by device
  capabilities, not node numbering.
- **Describe** a camera: driver/bus info, pixel formats, frame sizes, frame intervals
  (per-format [PF:9]), and the full control set — types, ranges, steps, defaults, current
  values, flags, menu items — including controls of types we do not recognize [PF:1].
- **Drive controls**: read, write (with read-back), guarded writes that disable the paired
  automation control first (D3), snapshot and restore of the whole control state (D4).
- **Capture photos**: format/resolution selection, settle policy (skip-frames/deadline)
  [PF:11], output as verbatim camera JPEG or PNG, EXIF-stamped with capture metadata (D6).
- **Capture video**: MJPEG remuxed to AVI in-process; raw Y4M as the escape hatch (D7).
- **Calibrate**: sessions that track per-control status and precision, plan and execute
  sweeps over control ranges with a photo per sample, score samples with built-in metrics,
  and persist everything as inspectable files an agent or human can review (D8, D9).
- **Four consumers of one library** (five deliverables counting the library): a direct CLI
  (`wch`), a daemon serving JSON-RPC (`wchd`), a CLI client for the daemon (`wchc`), and a
  browser web client served by the daemon. The direct CLI and the daemon client share one
  command surface (T4).

**The in-process constraint (owner's requirement).** The tool links Rust libraries and
performs the work in-process (multiple threads are fine); it does not orchestrate external
binaries at runtime. Build-time dependencies (bindgen, libclang, kernel UAPI headers) are
acceptable. Test-time oracles (ffprobe validating our AVI output in CI) are acceptable —
they check our output, they do not produce it.

**Non-goals (v1).**

- **Audio capture.** Every maintained Linux audio path (`alsa`, `cpal`) links LGPL
  `alsa-lib`, which the license constraint excludes; recorded as §8 item 2 rather than
  silently dropped. Video files carry no audio track in v1.
- **Transcoding.** No H.264/AV1 encoding in v1; the layered options and their license and
  patent postures are recorded in D7 and §7 so they are not re-derived.
- **Non-V4L2 backends.** The trait boundary (T1/T2) is designed for them; none ships in v1.
  The fake backend is a test instrument, not a product backend.
- **Remote/multi-host operation, TLS, non-loopback exposure.** The daemon is a local
  single-user service; hardening beyond D11's posture is out of scope.
- **Motorized-PTZ closed-loop tracking.** We expose pan/tilt/zoom controls and speeds; we
  do not implement tracking loops.
- **Cross-browser support.** The web client targets Chrome (§2.7); Firefox/Safari are
  best-effort and never a constraint on features or design.

### 1.1 The skill-to-operation map

Every teachable operation in `vendor/v4l2-webcam-skill/` becomes a first-class operation,
so an agent issues one tool call where the skill teaches a command sequence:

| Skill operation (manual commands) | Tool operation |
|---|---|
| `v4l2-ctl --list-devices` + per-node `--info` to find capture nodes | `list` (grouping + capture-node detection built in, D1) |
| `--list-formats-ext` | `info` (formats × sizes × intervals) |
| `--list-ctrls-menus` | `controls` (full model, D2) |
| `--get-ctrl` / `--set-ctrl` | `get` / `set` (read-back, D3) |
| discover auto/manual pairs, disable auto, set manual, restore | `set --guarded` (pairing table + empirical discovery, D3); `snapshot`/`restore` (D4) |
| ffmpeg one-frame capture with `-ss` settle | `photo` (settle policy, D5/D6) |
| ffmpeg `-vf hflip/vflip/transpose` flip and rotate | `photo --transform` (EXIF Orientation on verbatim JPEG, pixel-domain on PNG/re-encode — D6) |
| ffmpeg `-t` video capture | `record` (D7) |
| draft-calibration.sh + uniform-sampling.sh + status tracking | `calibrate start/plan/sweep/status/select/apply/list` (D8) |
| `fuser` busy diagnosis + `fuser --kill` | typed `Busy` error naming holders (D13); `terminate-holder` as a distinct explicit command (D10, §5) |
| `lsusb` missing-camera diagnosis | empty `list` output carries the driverless-USB-camera hint (D1) |

### 1.2 The probe record (PF registry)

PF:1–12 were measured 2026-08-07, during design; PF:13–16 were measured 2026-08-08,
during implementation (PF:13 while P0 was still open, the rest during P1/P2), on the
same host and hardware — their full transcripts live
in the implementation notes, which is where a new finding always lands first (rubric
rule 5, AGENTS.md rule 4) before a revision of this document absorbs it. These are
load-bearing citations, not anecdotes.

- **PF:1 — The `v4l` crate panics on modern control types.** `query_controls()` unwraps a
  conversion that lacks type 0x0107 (`RECT`); the Chicony exposes `Region of Interest
  Rectangle` (0x00981ae1, elem_size 16) on kernel 7.0 and enumeration panics
  (v4l-0.14.0/src/control.rs:172). The menu-item conversion has the same shape.
- **PF:2 — Menu indices are sparse.** Chicony `Auto Exposure` has items {1, 3}; OBSBOT has
  {0, 1, 3}. `VIDIOC_QUERYMENU` returns EINVAL on the holes. Item names differ per device;
  "manual mode" discovery must read names, never assume indices.
- **PF:3 — INACTIVE tracks auto/manual pairing live, both directions.** Setting OBSBOT
  `white_balance_automatic=1` flips `white_balance_temperature`'s flags 0x1000→0x1010
  (INACTIVE set); clearing it flips them back. Pairing is *empirically discoverable* by
  toggling an automation control and diffing INACTIVE across the control set.
- **PF:4 — Current values can sit outside the declared range.** OBSBOT `Zoom, Continuous`:
  range [-100..100], current value 245.
- **PF:5 — Defaults can sit outside the declared range.** OBSBOT `Power Line Frequency`:
  menu range [0..2], default 3.
- **PF:6 — Out-of-range writes are silently clamped, not refused.** `S_CTRL` with 99999 on
  a max=10000 control returns success and applies 10000. (The spec permits ERANGE; uvcvideo
  clamps.)
- **PF:7 — One USB device can host multiple logical cameras; nodes group by USB
  interface.** Chicony RGB = interface 3-4:1.0 (video0 capture + video1 metadata), Chicony
  IR = 3-4:1.2 (video2 GREY capture + video3 metadata), OBSBOT = 3-1:1.0 (video4+5). Media
  controller devices mirror the grouping 1:1. Capture vs metadata nodes are distinguished
  by `device_caps` (`VIDEO_CAPTURE` vs `META_CAPTURE`) — the skill's "lowest-numbered
  node" heuristic is replaced by this check.
- **PF:8 — Serial numbers are unreliable identity.** The Chicony reports serial "0001";
  the OBSBOT reports none at the interface parent.
- **PF:9 — In-process capture works; format lists are per-pixel-format.** mmap streaming
  via the `v4l` crate produced a valid 1920×1080 MJPEG frame (SOI/EOI intact) in 2.0s on
  the OBSBOT (including a 10-frame settle) and 0.48s on the Chicony. The OBSBOT offers
  MJPG up to 3840×2160 while YUYV stops at 640×480 (USB bandwidth) — frame-size
  enumeration must be nested under pixel format.
- **PF:10 — Build requirements.** `v4l` 0.14 + `v4l2-sys-mit` is pure-ioctl (no runtime C
  library) but runs bindgen at build time: libclang + kernel UAPI headers are build deps.
  Compiles clean in an edition-2024 workspace.
- **PF:11 — Early frames are unsettled.** Frames immediately after STREAMON are
  dark/miscolored before AE/AWB converge; a settle policy is required for photos.
- **PF:12 — Read-only controls exist, and the decoded flag set must expect growth.**
  Chicony `Privacy` is READ_ONLY. Most integer controls on this kernel carry flag bit
  0x1000 — `V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX`, which recent kernels set widely (it arrived
  with the same kernel work as the RECT/ROI support behind PF:1) and older references do
  not list. Flags are carried as raw bits plus a decoded known set, so next year's bit is
  data, not a surprise.
- **PF:13 — `bus_info` is per-USB-device, not per-logical-camera.** Both Chicony logical
  cameras report the identical `usb-0000:00:14.0-4`; only the sysfs USB *interface* path
  (`3-4:1.0` vs `3-4:1.2`) tells the RGB camera from the IR one. The card names differ,
  but that is vendor courtesy, not a guarantee. Grouping and
  `CameraFingerprint::bus_path` therefore use the interface path, never `bus_info` — a
  fingerprint built on `bus_info` would let `calibrate apply` replay an IR session onto
  the RGB sensor.
- **PF:14 — A UVC camera's VideoStreaming interface never has a V4L2 binding.**
  `uvcvideo` hangs the capture nodes off the VideoControl interface; the sibling
  VideoStreaming interface is claimed by the same driver but carries no `video4linux/`
  directory of its own. A per-interface "USB camera without a driver" scan therefore
  indicts every healthy camera on the machine; the question is asked per USB *device*
  (at least one video-class interface, none of them bound).
- **PF:15 — `ENOTTY` is how a node says "I do not implement that ioctl", and it
  terminates enumeration.** V4L2 has no count-first call; every enumeration walks an
  index until the kernel refuses, and the refusal is `EINVAL` *or* `ENOTTY` — the same
  metadata node answers `ENOTTY` for the control ioctls and `EINVAL` for `ENUM_FMT`'s
  missing indices, and a caller cannot predict the split from the node's capabilities.
  Both end a walk; `ENOTTY` from `QUERYCAP` still means "not a V4L2 device" and stays an
  error.
- **PF:16 — `little_exif` cannot write EXIF into a JPEG that uses restart intervals.**
  Its whole-file marker walk reads entropy-coded bytes (restart markers `FF D0`–`FF D7`,
  `FF 00` stuffing) as segment lengths — roughly one Chicony frame in three failed, at a
  rate that varied with the scene. The fix is structural: build the APP1 segment without
  sight of the file and splice it walking only the header, stopping at `SOS` (D6, E6).

## 2. Architecture

### 2.1 System overview

```
            ┌────────────────────────────  one command surface (T4)  ───────────────────┐
            │                                                                           │
  wch (CLI) ──▶ engine (in-process) ──▶ CameraBackend trait (T1/T2) ──▶ webcam-handler-v4l2 ──▶ /dev/video*
            │        ▲                                              └─▶ webcam-handler-fake ──▶ device-profile corpus
 wchc (CLI) ──▶ jsonrpsee client ─┐                                                 (captured from real cameras)
  web client ──▶ WS / HTTP ───────┴─▶ wchd: jsonrpsee server ──▶ engine (same trait objects)
                                        │  UDS always · TCP opt-in (D11)
                                        └─▶ axum: static web assets · MJPEG preview · WS
```

| Component | Crate | Role |
|---|---|---|
| Schema | `webcam-handler-schema` | every shared type: camera/control/capability model, session state, error codes, API DTOs (serde + schemars derives) [pure] |
| Imaging | `webcam-handler-imaging` | JPEG decode (zune-jpeg), YUYV→RGB (yuv), PNG/JPEG encode (image), AVI/MJPEG muxer (ours), Y4M writer, quality metrics (imageproc) — bytes in, bytes out [pure] |
| Engine | `webcam-handler-engine` | camera manager, guarded-set planner, snapshot/restore, capture orchestration, calibration engine, session store; takes backends as values |
| V4L2 backend | `webcam-handler-v4l2` | T1/T2 impl: sysfs+devnode enumeration, raw QUERY_EXT_CTRL control layer, raw-ioctl mmap streaming, uevent hotplug [Linux I/O] |
| Fake backend | `webcam-handler-fake` | T1/T2 impl: scriptable in-memory cameras + device-profile replay (T3) [pure] |
| API | `webcam-handler-api` | the jsonrpsee `#[rpc(server, client)]` trait over schema types + the error-code registry (T5) [no web stack; tokio arrives via jsonrpsee — N5, §2.8] |
| Direct CLI | `webcam-handler-cli` (bin `wch`) | command core → engine, in-process |
| Daemon | `webcam-handler-daemon` (bin `wchd`) | jsonrpsee server on UDS (+ opt-in TCP), axum web serving, MJPEG preview, subscriptions, systemd integration |
| Daemon client | `webcam-handler-client` (bin `wchc`) | command core → generated jsonrpsee client |
| Web assets | `webcam-handler-web` | vanilla HTML/JS/CSS, rust-embed'ed into `wchd` |
| Test kit | `webcam-handler-testkit` | device-profile corpus loader, synthetic image fixtures, container oracles — dev-dependency only |

**Request lifecycle (photo, via daemon).** `wchc photo cam:obsbot --settle 1s -o
out.jpg` (an unambiguous id prefix, D1) → generated client sends `photo` over UDS → daemon routes to the per-camera actor
(D12) → actor snapshots requested controls if `--guarded` writes are pending, negotiates
format (D5), streams until the settle policy is satisfied [PF:11], takes the frame →
imaging emits verbatim JPEG or PNG (D6) → EXIF stamped (capture time, camera identity,
control values in effect) → bytes returned or written server-side per the request's sink →
the actor restores anything it changed (D4) → response carries `{requested, applied}` for
every control it touched (D3).

**Concurrency model (D12).** The engine owns each open camera through a dedicated OS
thread (the *camera actor*): V4L2 ioctls and DQBUF block, so each camera gets one blocking
thread with a command channel; the daemon's async tasks and the direct CLI both talk to the
same actor API. One actor per camera serializes device access by construction — there is no
"two writers negotiate" state. Streaming fan-out (preview + capture) happens inside the
actor via a latest-frame `watch` channel, so a slow HTTP consumer drops frames and never
backpressures the device.

### 2.2 The domain model — decisions

**D1 — Camera identity and grouping.** A *camera* is a group of device nodes sharing a USB
*interface*, keyed on the sysfs interface path — never on `QUERYCAP bus_info`, which both
Chicony logical cameras report identically [PF:7, PF:13]. (For non-USB V4L2 devices: a
media controller device / driver-reported bus info.) The capture node is the group member whose `device_caps` contain
`VIDEO_CAPTURE` and whose format list is non-empty; `META_CAPTURE` nodes are recorded but
never streamed. Identity has two tiers:

- `CameraId` — the name RPC calls and CLI arguments use. Grammar: `cam:<card-slug>[-<n>]`,
  where `card-slug` is the querycap card name through the slug transform (D2's, with `-`
  as the separator), and `-<n>` (n ≥ 2) is appended on collision in enumeration order.
  Stability scope, stated exactly: reproducible across runs while the attached-device
  topology is unchanged; stable for the lifetime of one engine instance even across
  replug (the id follows the fingerprint); never persisted as identity — that is the
  fingerprint's job. One ambiguity is closed by rule because the seed hardware exhibits
  it: "OBSBOT Tiny 3" slugs to `obsbot-tiny-3`, which looks like a collision form of
  `obsbot-tiny` — so a natural slug always wins its own name, and a collision ordinal
  increments until it collides with nothing (natural slugs included); the
  trailing-digit case is a committed slug fixture. Commands accept any unambiguous
  prefix (`cam:obsbot` resolves if only one id starts that way), so long derived slugs
  cost agents nothing.
- `CameraFingerprint` — best-effort stable across replug/reboot: bus path (the USB
  *interface* path, `3-4:1.2`-shaped — the only identity fine enough to tell the Chicony
  IR camera from the RGB one [PF:13]), USB VID:PID,
  card name, serial *when the device provides a distinguishing one* [PF:8]. Calibration
  sessions record the full fingerprint and match conservatively; a fingerprint mismatch on
  `apply` is a refusal with the differing fields named, not a warning.

Node numbering (`/dev/video0` vs `video1`) is never load-bearing. An empty enumeration is
diagnosed, not shrugged at: `list` with zero cameras scans sysfs for USB video-class
*devices* none of whose interfaces has a video4linux binding, and reports "USB camera
present without a V4L2 driver" when it finds one — the skill's `lsusb` triage, built in
(§1.1). Per *interface* the same scan would indict every healthy UVC camera on the
machine, because `uvcvideo` never binds nodes to the VideoStreaming half [PF:14]. The
hint reaches clients through T1's `diagnose` (§2.3), so nothing above the backend seam
has to know which backend it is holding.

**D2 — The control model: represent, don't reject.** A control descriptor carries: numeric
id, name, slug (the `v4l2-ctl` spelling agents already know; the transform is pinned
because two readings of "lowercase and underscore" diverge on real names: lowercase, keep
`[a-z0-9]` runs, collapse every other run to a single separator, trim — `_` for control
slugs, so `Zoom, Continuous` → `zoom_continuous` and `Region of Interest Rectangle` →
`region_of_interest_rectangle`; slug fixtures in the committed profiles pin it), type,
range `{min, max, step}` as i64, default, flags (raw u32 + decoded known set [PF:12]), menu items as a **sparse map** index→name-or-value [PF:2], element count and
size for array/compound controls, and the current value *as read, unvalidated* [PF:4].
Control types are a closed enum with an `Unknown { raw: u32 }` variant carrying payload
size — a RECT control enumerates, displays, and round-trips as opaque bytes even though v1
cannot interpret it [PF:1]. Defaults and current values outside the declared range are
reported as measured, flagged in output, and never "corrected" [PF:4, PF:5]. The control
enumeration loop is ours (raw `VIDIOC_QUERY_EXT_CTRL` with `NEXT_CTRL|NEXT_COMPOUND`,
`VIDIOC_QUERYMENU` over min..=max tolerating holes); the `v4l` crate's high-level
`query_controls()` is not called (PF:1 is a panic, and a library that can be panicked by
plugging in a webcam is not a library).

**D3 — Writes read back; guarded writes handle automation.** `set` always re-reads after
`S_EXT_CTRLS` and returns `{requested, applied}`; a clamp [PF:6] is a *warning-carrying
success*, not an error, because the driver accepted it — but the caller always learns what
actually happened. A **guarded set** of a manual control first resolves its automation
partners and disables them, reporting every change it made. Pairing resolution is layered:

1. **The declared table** (data, in `webcam-handler-schema`): the well-known UVC pairs
   (`focus_absolute` ↔ `focus_automatic_continuous`; `exposure_time_absolute` ↔
   `auto_exposure` + `exposure_dynamic_framerate`; `white_balance_temperature` ↔
   `white_balance_automatic` — with `white_balance_temperature_auto` as the alternate
   older spelling the skill records), including menu-valued automation controls where
   "manual" is found by menu-item *name* [PF:2].
2. **Empirical discovery** (`controls --discover-pairs`, and automatically at calibration
   start): drive each automation-shaped control and diff INACTIVE flags across the set
   [PF:3]; discovered pairs are recorded in the device profile with provenance
   `measured`, and trump the table on conflict (E1). Three rules govern the probe, each
   paid for by a defect the P2 review confirmed [notes, E4]: **a menu is not a switch**
   — every alternative of a menu-valued automation control is tried, never just the
   lowest index (a three-item `auto_exposure` resting on `Aperture Priority Mode` has
   two other positions, and a pair may reveal itself on only one of them); **residue is
   isolated** — a candidate that cannot be undone must not leave state a later
   candidate's diff is measured against, or the probe invents pairs and stamps them
   `Measured`; and **"off" is recorded per freed control** — one menu position can free
   one control and freeze another, so the off-position is a per-(automation, controlled)
   fact found by menu-item *name* [PF:2], never a single value inferred for everything
   the toggle moved.

**D4 — Snapshot and restore.** `snapshot` captures every writable control's current value;
`restore` replays it (automation controls first, then manual ones, so re-enabled automation
does not immediately overwrite a restored manual value — ordering is load-bearing and
tested). Sweeps and guarded operations wrap themselves in snapshot/restore by default; the
tool leaves the camera as it found it unless told to keep changes (`--keep`). Restore
reports per-control `{requested, applied}` like any write; INACTIVE controls are
restored on a second pass after their automation partner is handled. The outcome
vocabulary has **four** values, and which of them count as complete is load-bearing
[N9]: `Restored` (written back), `AlreadyCorrect` (nothing to do),
`OwnedByAutomation { control, automation }` — the control is INACTIVE because its
automation owner is back in charge, exactly as at snapshot time — and
`Unrestorable { reason }`, whose `StillInactive` reason means a control that was *ours*
at snapshot time is automation-owned now: a real change we could not undo.
`OwnedByAutomation` counts as **complete**: on any device whose INACTIVE flag follows
its automation control [PF:3] it is the ordinary result of every guarded write's
restore, and a report that cries failure on the common success is a report people stop
reading. Telling it apart from `StillInactive` requires deciding on the device's
*present* state as well as the snapshot's record — which is why restore reads both. (In
§5's terms: every sweep/guarded operation restores by default, and the R3 hardware
suites assert the restoration.)

**D5 — The capture pipeline.** Format negotiation prefers the requested
format/resolution/rate; the *negotiated* result is always reported (drivers adjust
silently, same doctrine as D3). Settle policy is explicit: `skip_frames(n)` (default 10
[PF:9, PF:11]) or `settle_for(duration)`, both bounded by a deadline. Size selection asks
the *range* the question: a stepwise or continuous frame-size entry answers with the
closest size it can actually deliver (`largest_within`), never collapsed to its maximum
corner first — a device offering 32..1920 in steps of two can deliver a requested 640×480
exactly, and answering 1920×1080 "as an adjustment" would be false [notes, E4]. Buffers are mmap'd;
the taken frame is copied out at `bytesused` and the stream torn down. Capture never
re-encodes what it doesn't have to: an MJPG frame destined for a `.jpg` sink is written
verbatim (E6).

**D6 — Photo outputs.** Supported source formats, stated closed: MJPG, YUYV, GREY (the
Chicony IR camera is a seed corpus device — grayscale is not optional), and NV12 (the
`yuv` crate covers it alongside YUYV); anything else is `FormatUnsupported { available }`
(D13). Sinks: `.jpg` = verbatim camera bytes when the negotiated format is MJPG
(byte-fidelity: what the camera emitted is what lands on disk), else re-encode; `.png` =
decode (zune-jpeg) or convert (yuv, or trivial widening for GREY) then encode; raw
`.ppm`/bytes for tooling. Orientation transforms (`--transform hflip|vflip|rot90|rot180|
rot270`, the skill's flip/rotate): on the verbatim-JPEG sink they are an EXIF Orientation
tag — zero re-encode, byte fidelity preserved (E6); on PNG/re-encode sinks they are
pixel-domain (the `image` crate's rotate/flip). EXIF (little_exif) stamps capture time,
camera fingerprint, negotiated format, and the control values in effect — a calibration
sample photo is self-describing without its session file. The stamp is a **header-only
splice** [PF:16]: `little_exif` builds the APP1 segment, which requires no sight of the
file, and `imaging::exif` inserts it after `SOI`, walking only header segments and
stopping at `SOS` — the library's own whole-file writer misreads entropy-coded bytes as
segment lengths on cameras that emit restart intervals, so it never sees our file. The
walk also treats the bitstream as device data (rubric B10): a header length running past
the buffer ends the walk instead of indexing past it, and the file still gets stamped.
Every photo path — verbatim
JPEG, PNG, and JPEG re-encode (the `image` crate's own MIT/Apache encoder; audit-verified
**not** the IJG-licensed `jpeg-encoder`) — is allowlist-clean.

**D7 — Video recording: the license-layered strategy.** v1 ships L0 only; the layers are
recorded so they are not re-derived:

- **L0 (ships, default): MJPEG → AVI, our muxer.** AVI is the canonical MJPEG container;
  no maintained AVI writer exists on crates.io (verified — the `avi` name is a reserved
  empty crate), and the format is frozen, so we own ~300 lines with an ffprobe round-trip
  oracle in CI. No encoder, no patent surface, no copyleft. Bounded by duration and size
  caps; index written on close; a crash leaves a recoverable `movi` stream (documented).
  AVI is a constant-frame-rate container and cameras are not: the header's rate is the
  negotiated frame interval, rewritten at close to the measured mean interval, and the
  CFR-vs-actual-delivery limitation is recorded here and in §3.3 — a G6 oracle criterion
  bounds declared vs wall-clock duration on a real capture.
  Raw fallback: Y4M (y4m crate) for YUYV/GREY cameras and pipeline use (mono and 4:2:x
  are both in the Y4M vocabulary) — enormous but exact. The muxer is MJPG-only by
  design; non-MJPG `record` requests get Y4M or `FormatUnsupported { available }`.
- **L1 (deferred): UVC H.264 cameras → MP4 remux** (h264-reader + a pure-Rust MP4 muxer).
  No encoder, real .mp4. Neither probe camera exposes H.264; lands when hardware exists to
  test against (E2: no capability code without a device that exhibits it).
- **L2 (deferred, non-default feature): AV1 via rav1e** (BSD-2, royalty-free) — the only
  fully-permissive in-process encode path; too slow for live 1080p30, honest scope is
  offline/timelapse.
- **L3 (recorded, not planned): openh264.** BSD-2 *code* license with **no patent grant**
  when compiled from source; Cisco's royalty coverage applies only to their downloaded
  binary. Any adoption is a non-default feature with the posture documented.
- **Rejected outright:** anything linking libx264/x265 (GPL), FFmpeg libraries (LGPL/GPL —
  no compliant build configuration exists under this constraint), GStreamer (LGPL),
  `minimp4`/`env-libvpx-sys`/`rust-webm` (MPL wrappers). §7 records details.

**D8 — The calibration model.** A *session* belongs to (camera fingerprint, task) and is a
directory (D9). Its state:

- **Goal and criteria**: free-text task ("read text from the DUT display") plus an ordered
  criteria list — recorded because the *selector* needs them, whether that selector is a
  human, an agent, or a metric.
- **Per-control status**, the closed vocabulary:
  `Untouched` → `AutoDisabled` (automation partner disabled, value parked) →
  `Sweeping { plan, done, total }` → `Calibrated { value, precision, score, selector }` |
  `Deferred { reason }` | `Blocked { reason }` (e.g. READ_ONLY [PF:12], or INACTIVE with
  no discoverable automation partner). `precision` is the final sampling step (the skill's
  "Calibration precision: 100"), so multi-pass refinement (coarse → fine) is representable.
- **Sweep plans**: `All`, `Uniform { step }`, `Log { points }`, `Explicit { values }`; the
  planner derives candidate values from the *measured* range and aligns them to the
  control's step; execution is guarded-set → settle → capture → score → record, one sample
  row per value: `{value, applied, photo, metrics, timestamp}` — `applied` because D3
  applies inside sweeps too [PF:6].
- **Scoring**: built-in metrics computed per sample — Laplacian-variance sharpness,
  clipped-highlight/shadow fractions, mean luma, RMS contrast (imageproc + ~30 lines of
  ours). Metrics *rank*; they do not *decide*. The `Calibrated` record names its
  `selector`: `metric:<name>`, `agent`, or `human` — an agent reviewing sample photos
  visually (the skill's flow) records its choice and the tool tracks it; nothing pretends a
  Laplacian knows what "text legible on the DUT" means.
- **Ordering and interaction**: the session holds an ordered control queue the caller may
  reorder between sweeps (the skill's step 10.5); interaction notes are free-text on the
  session, not modeled — v1 does not pretend to know that focus and exposure interact, it
  lets the operator record it.
- **Apply**: `calibrate apply` replays a session's calibrated values (automation-disables
  first, D4 ordering) against a fingerprint-matched camera — the skill's "calibration
  script" as data instead of Bash.

**D9 — Persistence: inspectable files, atomic writes, one lock.** Session layout under the
XDG state dir:

```
$XDG_STATE_HOME/webcam-handler/sessions/<fingerprint-slug>/<task-slug>/<uuidv7>/
  session.json     # schema_version, fingerprint, goal, criteria, control queue + statuses,
                   # sample index — pretty-printed, diffable, jq-able
  log.ndjson       # append-only event log; a torn last line is dropped on load
  photos/<control-slug>/<value>.jpg|png
```

Writes go through one audited `write_json_atomic` (tempfile in-dir → sync_all → rename →
fsync parent). Cross-process safety is one advisory `fd-lock` at the state-dir root: the
daemon holds it exclusively for its lifetime; a daemonless `wch` takes it per mutating
operation; `wch` finding it held reports "daemon owns the state (and likely the camera) —
use wchc" rather than corrupting or blocking (D13). Photo paths inside session files are
relative `camino` UTF-8 paths, so a session directory is relocatable as a unit. Every JSON
file carries `schema_version` from day one.

**D10 — One wire surface, one home (T5).** The whole daemon API is one jsonrpsee
`#[rpc(server, client)]` trait in `webcam-handler-api` over `webcam-handler-schema` DTOs. The daemon implements
the server half; `wchc` consumes the generated typed client; the direct CLI calls the same
methods on the engine through T4's executor abstraction — so a verb exists exactly once.
Methods (namespace `wch`): `list`, `info`, `controls`, `discover_pairs` (D3's empirical
pass), `get`, `set` (guarded flag), `snapshot`, `restore`, `photo`,
`record_start/stop/status` (progress by polling `record_status` — no recording
subscription in v1), `profile_capture` (T3), `terminate_holder { camera, pid }` (the
explicit kill, §5 — refuses if the pid no longer holds the device), `calibrate_*` (start,
plan, sweep, status, select, apply, list), `subscribe_events` (hotplug), and
`subscribe_calibration` (per-session progress). Binary results (photo/record) cross the
wire via a two-variant sink DTO in `webcam-handler-schema`: `ReturnBytes { format }`
(base64 in the JSON result) or `ServerPath { path }` (absolute UTF-8 camino path; clients
resolve relative paths against their own cwd *before* sending, so `-o out.jpg` means the
caller's directory in both `wch` and `wchc`). Errors cross the wire as the structured
registry (D13): `code` from a closed numeric range, `message` human, `data` typed. DTOs
derive `schemars::JsonSchema`; the build emits a JSON Schema bundle + OpenRPC document as
generated artifacts (documentation, not a second source of truth).

**D11 — Transports and the security posture.** The Unix socket
(`$XDG_RUNTIME_DIR/webcam-handler/wchd.sock`, directory 0700) is always served: filesystem
permissions are the auth model, which is correct for a per-user hardware daemon. TCP is
**opt-in** (`--http [addr]`, default `127.0.0.1:0` → report the bound port), serves the
web client (static assets + WS JSON-RPC + MJPEG preview `<img>` endpoint), and requires a
bearer token: generated per run and printed as a ready-to-open URL unless configured. The
full bind × token matrix, stated once (docs/7 G5 and the docs/9 token gate cite this
paragraph rather than paraphrasing): loopback + token is the default; token-less loopback
exists only behind one named explicit flag (`--http-insecure-loopback`); non-loopback
**always** requires the token — there is no flag that removes it — and additionally
prints a warning naming what it exposes (a live camera). Loopback + token because
loopback alone is not an auth boundary on a multi-user machine. A camera is a
privacy-sensitive device; the daemon's exposure posture errs closed.

**D12 — Concurrency and ownership** — see §2.1. One additional rule: the actor enforces
*exclusive streaming* (V4L2 allows one streamer per node); control reads/writes interleave
with streaming, but a second capture request queues or is refused with `Busy` per its
`wait` flag. The daemon never opens a camera until first use and closes on idle (configurable), so
`wchd` running does not itself block other applications from the webcam.

**D13 — The error vocabulary.** `webcam-handler-schema` defines the closed typed
registry — **nineteen variants** (v1's fourteen, four completions recorded as N4, and
one transitional recorded as N6); every variant carries what the caller needs to act:
`DeviceGone`, `Busy { holders: Vec<{pid, comm}> }` (diagnosed from `/proc/*/fd` — the
`fuser` replacement; killing is a
separate explicit operation, never a side effect), `PermissionDenied { path, hint }` (the
"add yourself to the `video` group" hint lives here, once), `ControlUnknown`,
`ControlReadOnly`, `ControlInactive { automation: Option<Slug> }` (names the automation
control to disable — actionable, from PF:3),
`FormatUnsupported { available }`, `SettleTimeout`, `FingerprintMismatch { fields }`,
`SessionConflict`, `IllegalTransition { from, op }` (the D8 state machine's refusals),
`SchemaVersionForeign { found, supported }` (the D9 versioned-load refusal),
`StoreLocked { holder }`, `HolderGone { pid }` (`terminate_holder`'s refusal when the
named pid no longer holds the device), `CameraUnknown { requested }` (a name that never
resolved — distinct from `DeviceGone`, a camera that *was* there),
`CameraAmbiguous { requested, candidates }` (D1's prefix resolution matching more than
one camera; carries every candidate), `DeviceIo { operation, errno, message }` (every
errno the typed variants do not claim — `EINVAL` on a format negotiation, `EIO`
mid-stream — typed with the operation and errno, because a stringly error crossing the
wire is a rubric finding), and `StorageIo { path, errno, message }` (the D9 fault menu's
filesystem failures — its own menu names "full disk", and nothing else covered it).

The nineteenth, `Unimplemented { operation, arrives_in }`, is **transitional** (N6): the
T1/T2 traits are total from P0 while the plan lands their methods across phases, so a
method whose phase has not arrived needs an answer that is neither a panic [PF:1], nor
the kernel's fault (`DeviceIo`), nor a capability lie (E3's whole subject). It names the
operation and the phase, says *this build* rather than *this device*, and is pinned to
the one `unimplemented_surface()` list, whose test the landing phase must edit. When
`watch` lands at P4 the list is empty and the variant is deleted — the exhaustive
`Error::kind()` match drives the removal.

The registry is **errors only**: a clamp
[PF:6] is not in it — it rides the write result as a typed warning on `{requested,
applied}` (D3), because a warning with an error code is a success nobody can distinguish
from failure. `ErrorKind` and its `ALL` are generated by the `closed_vocabulary!` macro
and `Error::kind()` is an exhaustive match, so a twentieth variant does not compile
until the round-trip, rendering, and RPC-code walks all know it. JSON-RPC error codes
map 1:1 from this registry in `webcam-handler-api`;
the CLI renders the same variants; nobody stringly-matches.

### 2.3 The backend contract (T1–T3)

**T1 — `CameraBackend`.** The pluggability seam the owner asked for:

```rust
pub trait CameraBackend: fmt::Debug + Send + Sync {
    fn kind(&self) -> BackendKind;                        // the closed vocabulary
    fn name(&self) -> &'static str { self.kind().as_str() } // derived; cannot disagree
    fn enumerate(&self) -> Result<Vec<CameraInfo>, BackendError>;
    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>, BackendError>;
    fn watch(&self) -> Result<Box<dyn HotplugWatch>, BackendError>; // add/remove events
    fn diagnose(&self) -> Vec<ListHint> { Vec::new() }    // absence, explained (D1) [N7]
}
```

(`kind` is the required identity method — `BackendKind` is the closed vocabulary the
factory matches walk — and `name` is derived from it so the two cannot disagree; the
shape is the shipped one, in place since P0.) `diagnose` exists because D1's
empty-enumeration diagnosis needs a channel, and both
alternatives were worse [N7]: putting the scan above the seam means something up there
matching on `BackendKind` — the second home §2.10 forbids — and returning hints from
`enumerate` makes "the cameras" and "why there might be fewer than you expect" one value
when they are two facts. The V4L2 backend overrides it with the PF:14 per-device scan
plus a `NodeUnreadable` hint for any device node it could not interrogate;
the fake inherits the empty default, which is the honest position for a backend
replaying a document — its enumeration is complete by construction, so it has no absence
to explain. It is not a general status channel: `HintKind` is a closed vocabulary with
**two** variants, each of which met the same bar — a design requirement not otherwise
satisfiable: `DriverlessUsbVideoDevice` is D1's diagnosis [PF:14], and `NodeUnreadable`
(added by the P1 review) marks a camera missing from the listing because a node would
not answer — surfaced as explained absence, because listing the camera with only the
nodes that answered would let a busy capture node read as "this camera cannot capture",
the availability-as-capability conversion E3 forbids. A third variant needs what these
two had. `ListHint::message` renders the sentence once, in `webcam-handler-schema`,
so the CLI and the daemon cannot describe the same finding differently.

**T2 — `Camera`.** Blocking, object-safe, minimal — the engine owns policy, the backend
owns mechanism:

```rust
pub trait Camera: fmt::Debug + Send {
    fn info(&self) -> &CameraInfo;
    fn formats(&self) -> Result<Vec<FormatInfo>, BackendError>;      // sizes × intervals nested [PF:9]
    fn controls(&self) -> Result<Vec<ControlDesc>, BackendError>;    // D2, never panics [PF:1]
    fn get(&mut self, id: ControlId) -> Result<ControlValue, BackendError>;
    fn set(&mut self, id: ControlId, v: ControlValue) -> Result<Applied, BackendError>; // D3 read-back
    fn start_stream(&mut self, req: &StreamRequest) -> Result<NegotiatedStream, BackendError>;
    fn next_frame(&mut self, deadline: Instant) -> Result<Frame, BackendError>; // owned bytes + meta
    fn stop_stream(&mut self) -> Result<(), BackendError>;
}
```

**The traits' home is `webcam-handler-schema`** — they are object-safe vocabulary over
schema values, which puts them below every backend and below the engine in the DAG:
backends depend on schema only (plus imaging, for the fake's
synthetic-frame generation — §2.8), the engine consumes `Box<dyn CameraBackend>` values it is
handed, and nothing in the engine names a backend (§2.8, §2.11). Everything above T2
(guarded sets, snapshots, settle policies, sweeps, sessions, capture sinks) is engine
code shared by every backend; everything below is per-backend. A backend never renders,
never persists, never decides policy. Traits take and return `webcam-handler-schema`
values only. One signature note: `next_frame`'s deadline is an opaque bound the backend
honors literally; settle *policy* (which frames to skip, when to give up) lives in the
engine on a steppable clock — the deadline is how the policy's decision reaches the
blocking read.

Two contract notes added by this revision, paid for at P2 and P1 respectively [notes,
E4 and N6]:

- **Dispatch belongs to the descriptor.** `set`'s choice of kernel mechanism — scalar
  write vs payload write — is decided by the descriptor's `HAS_PAYLOAD` flag, exactly as
  reads decide, never by the caller's `ControlValue` variant. A `Bytes` value aimed at a
  scalar control is a typed refusal; dispatched on the value instead, it plants a heap
  address in the ioctl union, and `uvc_ctrl_set` clamps the low bits of that address
  into range and reports an ordinary adjustment — on a PTZ camera, a motor driven to its
  limit by an allocator. The fake refused this input all along; the divergence was the
  E5 resemblance claim failing in the direction that matters, which is how it was found.
- **The traits are total from P0** even though the plan lands their methods across
  phases: a method whose phase has not arrived answers `Error::Unimplemented` (D13,
  transitional) — never a panic, never a `DeviceIo`, never an empty capability answer.

**T3 — The device profile.** A JSON capture of everything a backend can enumerate about
a camera, in **two sections whose comparison semantics differ**: the *invariant*
description — identity, nodes, formats/sizes/intervals, the full control set with menus,
ranges and non-volatile flags, measured automation pairs (D3) — and the *state* block —
current control values and the INACTIVE-class flag snapshot, which change with use
[PF:3, PF:4] and are compared loosely or not at all (re-capturing a profile after a
sweep must not read as corpus drift). Provenance (`captured_at`, kernel, tool version)
rides outside both. `wch profile capture` emits one from live hardware; the committed corpus
(`corpus/profiles/`) holds profiles of real devices — the three probe-era cameras seed
it —
and `webcam-handler-fake` replays them: enumeration, control graph (including INACTIVE coupling
simulation [PF:3], clamping [PF:6], sparse menus [PF:2]), and synthetic frames whose
image content responds to control values (brightness shifts luma, focus blurs a test
pattern) so calibration logic is testable offline end-to-end. E5 applies: the fake's
behavioral claims are resemblance-tested against the profile it replays.

### 2.4 The engine

The engine is deliberately thin around a few pure cores:

- **The pairing planner** (D3): pure — `(controls, declared_table, measured_pairs, target
  set) → ordered write plan`. Property: the plan never writes a manual control while its
  automation partner is enabled; the inverse fixture (a plan that would) must be
  constructible and rejected.
- **The settle policy** (D5): pure state machine over frame timestamps.
- **The sweep planner** (D8): pure — `(ControlDesc, SweepSpec) → Vec<i64>`; total,
  clamped to measured range and step; never emits an empty plan without a typed reason.
- **The session state machine** (D8): pure transitions; illegal transitions are typed
  errors (you cannot `select` on a control that never swept; you cannot `apply` a session
  with zero calibrated controls without `--partial`).
- **The metrics** (D8): pure functions `&GrayImage → f64`, fixture-tested against
  synthetic images with known ordering (a blurred fixture scores below its sharp original
  — both directions).

The imperative shell around them: camera actors (D12), the session store (D9), sinks.
Doctrine held from the predecessor project: pure cores take values; seams exist only in
the shell, and each shell seam has a real implementation and a scriptable double
(§2.9).

### 2.5 The V4L2 backend

- **Enumeration**: scan `/sys/class/video4linux/*`, group by USB interface path (parent
  device link), read names from sysfs, open nodes for `QUERYCAP` `device_caps`
  [PF:7]. No udev dependency (LGPL, §2.8); identity fields come from sysfs + querycap.
- **Controls**: raw `VIDIOC_QUERY_EXT_CTRL`/`QUERYMENU`/`G_EXT_CTRLS`/`S_EXT_CTRLS`
  through the `v4l` crate's public ioctl escape hatch (`v4l::v4l2::{open, ioctl}` +
  `v4l::v4l_sys` types — validated during probing). The crate's own control layer is
  bypassed entirely (PF:1). Every index-walked enumeration accepts **either** `EINVAL`
  or `ENOTTY` as its terminator [PF:15], through the one
  `sys::ioctl::call_enumerating` home — a metadata node implements `ENUM_FMT` but not
  the control ioctls, and a metadata-only camera is a shape this project deliberately
  supports (listed, with streaming a typed refusal). `ENOTTY` from `QUERYCAP` does not
  go through that path and stays an error: "not a V4L2 device".
- **Streaming**: ours, end to end — `S_FMT`/`S_PARM`/`REQBUFS`/`QUERYBUF`/`QBUF`/
  `DQBUF`/`STREAMON`/`STREAMOFF` as raw ioctls through the same escape hatch, an owned
  mmap type (`sys::mmap`) holding each buffer's mapping lifetime, and a poll-bounded
  `DQBUF` (`sys::wait`); format negotiation surfaces the negotiated result (D5). v1
  planned to ride the `v4l` crate's mmap `Stream` (validated during probing, PF:9); the
  P2 implementation kept only the crate's `v4l2::{open, ioctl, mmap}` wrappers and its
  bindgen types, and the crate's io layer is unused.
- **Hotplug**: our own AF_NETLINK uevent socket (~30 lines; `kobject-uevent` 0.2 is a
  *parser* — its netlink-sys dependency is dev-only, audit-verified) with `kobject-uevent`
  parsing the packets, filtered to subsystem `video4linux`, debounced, re-enumerated on
  event — no libudev.
- **Control-change events** (`VIDIOC_SUBSCRIBE_EVENT`): deferred; the bindings already
  carry the structs, the wrappers are ~100 lines when a consumer exists (§8 item 4).
- **Busy diagnosis** (D13): on EBUSY, scan `/proc/*/fd` for the device node to name
  holders; degrade gracefully when /proc is restricted.

**The unsafe boundary** (norms transferred from the vmcell rubric, which paid for them —
its inline re-declaration of a kernel struct was a 22-byte out-of-bounds write that
tested green because the bytes landed in padding):

- **One module holds every `unsafe` block**: `crates/backends/v4l2/src/sys/`. Every
  *other crate* is `#![forbid(unsafe_code)]` at its root; within `webcam-handler-v4l2` —
  where a crate-root forbid is impossible by construction — confinement to `src/sys/` is
  enforced by the `unsafe-scope.sh` gate, which derives the allowed path from the tree
  (docs/9). The uevent socket code lives inside the same module for the same reason.
- **No hand-declared kernel structs.** Struct layouts come from `v4l2-sys-mit`'s bindgen
  output against the build host's real UAPI headers. If a definition must ever be
  hand-carried (a struct the bindings lack), it gets `const`-asserted `size_of` and
  per-field offset tests against the generated one — re-declaring inline is banned.
- **`// SAFETY:` proves the actual obligation** of each block — pointer validity and
  size for the ioctl at hand, initialized-union-field choice for `querymenu`, mmap
  lifetime for buffer access — one obligation per block
  (`clippy::multiple_unsafe_ops_per_block` denied); a false safety claim is a defect
  even when the code works.
- **Device-derived numbers are validated before use**: `bytesused` clamped to the mapped
  buffer length before slicing (a lying driver must not become an OOB read), menu index
  ranges bounded, control payload sizes checked against `elem_size × elems`, and every
  wire integer converted with `try_from`, never `as` (cast lints denied in this crate).
- **Miri runs the unsafe-adjacent pure units** — the raw-struct→`ControlDesc` decoding
  is written as a pure function over captured bytes precisely so Miri can execute it
  without a device (docs/9 commissions the job).

### 2.6 The daemon

jsonrpsee 0.26 server mounting the T5 trait; UDS via the tower-service-over-`UnixListener`
glue (server) and a small client transport (~200 lines, modeled on reth's production IPC
adapter — vendored knowledge, not a git dependency); axum 0.8 alongside for the web
client: rust-embed'ed static assets, WS endpoint speaking the same JSON-RPC, and the MJPEG
preview route (`multipart/x-mixed-replace`, fed from the actor's latest-frame watch
channel so slow clients drop frames). Shutdown discipline: SIGTERM and SIGINT drive the
same path — cancel long-lived streams via `CancellationToken` (an open MJPEG tab must not
hang shutdown), drain, close the store lock, `sd_notify(STOPPING)`. systemd integration is
`sd-notify` (READY once both listeners are up, STATUS with camera count, optional
watchdog) and `listenfd` socket activation; the daemon never self-daemonizes. Logging:
`tracing` everywhere; fmt layer foreground, journald layer (pure-Rust protocol, no
libsystemd) under systemd.

### 2.7 The clients

**The command core (T4).** Its own crate, `webcam-handler-cli-core` (`crates/cli-core/`)
— committed to up front, because the module-of-`wch`-reused-by-`wchc` alternative would
give `webcam-handler-cli` a library target and drag the engine and the V4L2 backend into
`wchc`'s link graph, which the thin-client wall (T6, §2.8) exists to forbid. It defines the clap
command tree, argument types (humantime durations, control `slug=value` pairs), rendering
(comfy-table for listings, indicatif for sweeps, anstream color discipline, `--json`
emitting schema DTOs verbatim), and an executor trait with two impls living in the
binaries: in-process engine (`wch`) and generated RPC client (`wchc`). A verb, its flags,
and its rendering exist once; the two binaries differ only in executor and connection
bootstrapping. `wch` refuses politely when the daemon holds the state lock (D9/D13).

**The web client.** Vanilla ES modules, no build step, no npm, no CDN (assets embed;
external fetches would violate both the offline posture and the license inventory): a
~50-line JSON-RPC-over-WebSocket helper, `<img>` for MJPEG preview, controls rendered
from the `controls` DTO (range → slider, menu → select with sparse indices [PF:2], flags
surfaced), calibration session view fed by the subscription. If a UI library ever earns
its keep it gets vendored under `webcam-handler-web/vendor/` with its license file, and §2.8's
inventory learns it. The browser half is CI-tested in a real headless Chromium via the
pinned Playwright rung (§3.1 R1-web). **Browser support policy (owner ruling): Chrome is
the supported target.** Modern platform APIs are used freely at Chrome's feature level; Firefox and
Safari compatibility is welcome when free but never justifies added complexity, feature
reduction, or a compatibility layer — a Firefox/Safari-only defect is recorded, not
necessarily fixed.

### 2.8 Workspace, dependencies, licenses

```
webcam-handler/
  Cargo.toml            # workspace, resolver 3, edition 2024, rust-version pinned
  crates/
    schema/             # webcam-handler-schema  [pure: serde, schemars, thiserror, camino, jiff,
                        #              uuid; home of the T1/T2 traits and the BackendKind vocabulary]
    imaging/            # webcam-handler-imaging [pure: zune-jpeg, yuv, image(png,jpeg only), png,
                        #              little_exif, imageproc, y4m; the AVI muxer is ours]
    engine/             # webcam-handler-engine  [schema + imaging + tempfile + fd-lock + tracing;
                        #              consumes Box<dyn CameraBackend>, names no backend]
    backends/fake/      # webcam-handler-fake    [pure; dev + test instrument, ships for `wch --backend fake`]
    backends/v4l2/      # webcam-handler-v4l2    [v4l (default features ONLY), kobject-uevent, libc, tracing]
    api/                # webcam-handler-api     [jsonrpsee macros + schema]
    cli-core/           # webcam-handler-cli-core [T4: clap tree, rendering, executor trait]
    cli/                # webcam-handler-cli     (bin wch)  [cli-core + engine + both backends; holds a
                        #              backend factory match]
    priv/               # webcam-handler-priv    (bin wch-priv) [dev-only privileged helper (§2.13);
                        #              never a dependency of any product crate — gate-asserted]
    client/             # webcam-handler-client  (bin wchc) [cli-core + jsonrpsee client; links no backend]
    daemon/             # webcam-handler-daemon  (bin wchd) [engine + both backends + factory match +
                        #              jsonrpsee server, axum, tower-http, rust-embed, tokio,
                        #              tokio-util, sd-notify, listenfd, tracing-journald]
    web/                # webcam-handler-web     [rust-embed asset crate; vanilla JS inside]
    testkit/            # webcam-handler-testkit [dev-only: corpus loader, synthetic fixtures, oracles]
  xtask/                # webcam-handler-xtask: completions (clap_complete), man pages (clap_mangen),
                        #              schema/openrpc emit
  corpus/profiles/      # committed device profiles (T3) — the three probe-era cameras seed it; file
                        #              names are capture-time human-chosen (provenance inside binds
                        #              them to fingerprints)
  corpus/images/        # synthetic image fixtures ONLY (generated patterns; never camera frames — §5)
  vendor/v4l2-webcam-skill/  # the manual workflow this tool replaces (§1.1); read-only reference
  docs/  scripts/gates/
```

**Naming convention (owner ruling):** every package name carries the full
`webcam-handler-` prefix; directory names stay short (`webcam-handler-engine` lives in
`crates/engine/`); library targets use bare `lib.name`s (`schema`, `engine`, …) so
in-code paths stay readable — the same package-prefix/bare-lib split the predecessor
uses. Binary target names are the user-facing commands `wch`, `wchd`, `wchc`. Doc
shorthand of the form `webcam-handler-engine::store` names the package; the in-code path
is `engine::store`.

Dependency edges, stated explicitly (an arrow means "is depended on by"; the shorthand
version of this list was ambiguous enough to review):

- `schema` ← everything (it holds the types, the T1/T2 traits, and `BackendKind`).
- `imaging` ← `engine` (and `fake`, for synthetic-frame generation).
- `engine` ← `cli`, `daemon` (the two composition roots) — **not** ← any backend; the
  engine consumes `Box<dyn CameraBackend>` values the roots construct.
- `fake`, `v4l2` ← `cli`, `daemon` only. Backends depend on `schema` (+ `imaging` for
  `fake`); nothing else depends on backends.
- `api` ← `client`, `daemon`; `cli-core` ← `cli`, `client`; `web` ← `daemon` only;
  `testkit` is dev-dependency-only (gate-asserted).

Purity walls (T6): `schema`, `imaging`, `fake`, `cli-core` link no tokio/axum/hyper.
`webcam-handler-api` carries its own wall — **no axum, no hyper, no tower-http; tokio
allowed** [N5]: a crate holding one `#[rpc(server, client)]` trait links tokio
necessarily (`jsonrpsee-core` activates it from both its client and server features —
measured across four feature sets, table in N5), and of the three ways out, splitting
the trait makes two wire surfaces (what D10/T5 exists to prevent), hand-rolling JSON-RPC
is §7's shape of last resort, and narrowing the wall to exactly what it was protecting
costs least. What it was protecting — "only `daemon` links the web stack" — is intact
and gate-asserted for `api` specifically (measured at adoption: no axum, no hyper, no
tower in its tree); re-run N5's measurement on any jsonrpsee bump, and delete
the exemption if a version makes the original wall satisfiable. Only `daemon` links the
web stack; `client` links no backend and no
engine (the thin-client property). Those are **linkage** walls, gate-asserted from
`cargo metadata` (docs/9). The *behavioral* halves — the pure crates touch no filesystem
outside explicit inputs, only `v4l2` touches `/dev` and `/sys`, only `engine` and the
composition roots touch the state dir, and `api` never *starts* a runtime or spawns a
task (linkage cannot see that half of N5) — are review-held, stated as such so the gate's
green is not read as covering them (docs/9 records the limit). The backend trait means
`engine` never names V4L2; a grep gate enforces that one (docs/9).

**License allowlist** (cargo-deny, enforced from P0): MIT, Apache-2.0, BSD-2-Clause,
BSD-3-Clause, ISC, Zlib, 0BSD, MIT-0, Unlicense, CC0-1.0, Unicode-3.0 (transitive
`unicode-ident`), plus `Apache-2.0 WITH LLVM-exception` (precautionary: rustix offers it
as one OR-alternative and already satisfies plain MIT/Apache — the entry exists so a
future dependency carrying it alone resolves by decision, not surprise). **Named bans**
with the reason on the entry: `v4l-sys` (links LGPL libv4l; the `v4l` crate's `libv4l`
feature that pulls it is the feature-posture gate's subject — docs/9 owns that half),
`v4l2-sys` (the LGPL-3.0 near-namesake of the MIT `v4l2-sys-mit`), `udev`/`libudev-sys`/
`tokio-udev` (LGPL libudev), `libcamera`/`libcamera-sys` (LGPL libcamera), `alsa`/
`alsa-sys`/`cpal` (LGPL alsa-lib), `colored` (MPL-2.0), `minimp4` (MPL-2.0 wrapper),
`env-libvpx-sys`/`vpx-encode`/`webm` (MPL wrappers), `ffmpeg-next`/`ffmpeg-sys-next`
(links LGPL/GPL FFmpeg), `x264`/`x265` bindings (GPL), `dssim` (AGPL), `jpeg-encoder`/
`turbojpeg`/`mozjpeg` (IJG term — §8 item 1; the default stack does not need them), and
`option-ext` (MPL-2.0, reached transitively through `directories`/`dirs-sys` — the ban
that keeps N2's drop from silently reverting). TLS
features stay off everywhere (localhost/UDS daemon): this also keeps `webpki-roots`
(CDLA-Permissive-2.0, off-allowlist) out of the tree — add it to the allowlist only if TLS
ever becomes real.

Core picks, as verified 2026-08-07 (versions are pins-at-adoption, not commitments):
`v4l` 0.14 (MIT; semi-dormant — pinned, wrapped behind T1/T2, with `v4l2r` 0.0.8 (MIT,
active, AOSP-integrated) recorded as the migration target if it dies), `kobject-uevent`
0.2 (MIT), tokio 1.x / axum 0.8 / tower-http 0.7 / tokio-util (MIT), jsonrpsee 0.26 (MIT;
0.x — minor pinned workspace-wide), rust-embed 8 (MIT; solo maintainer on a self-hosted
forge — reviewed on bump, `include_dir` fallback), zune-jpeg 0.5 (MIT/Apache/Zlib), yuv
0.8 (BSD-3/Apache), image 0.25 `default-features=false, features=["png","jpeg"]`
(MIT/Apache; the default `avif` feature drags rav1e — never enable by accident), png 0.18,
imageproc 0.27 (MIT), little_exif 0.6 (MIT/Apache; builds the APP1 bytes only — our
splice writes them, and the library's own JPEG writer never sees a camera file [PF:16]),
y4m 0.8 (MIT), clap 4 + complete +
mangen, comfy-table 8 (MIT), indicatif 0.18 (MIT), anstream/anstyle (MIT/Apache),
thiserror 2 / anyhow 1, tracing + tracing-subscriber + tracing-journald (MIT, no
libsystemd), serde/serde_json, toml 1 (config), schemars 1 (MIT), tempfile
3, fd-lock 4, uuid 1 (v7), jiff 0.2 (Unlicense/MIT; RFC 3339 strings on disk make it
swappable), camino 1, humantime 2, sd-notify 0.5 + listenfd 1 (pure-Rust systemd
protocols), kamadak-exif 0.6 (BSD-2, **dev-only**: the independent EXIF reader that
verifies what little_exif wrote — a gate-commissioned test oracle gets its §2.8 entry at
commissioning time, docs/9). `directories` was dropped before the scaffold settled [N2]:
it drags MPL-2.0 `option-ext`, the license gate caught it on its first run, and the tool
needs exactly two XDG paths on one platform — `engine::paths` owns them in ~thirty
lines, and the transitive culprit is on the ban list so the drop cannot silently revert.
Test-time external tooling, outside the shipped license
inventory but named here so it is chosen once: ffprobe/mpv as container oracles, and a
pinned Playwright + Chromium for the browser rung (§3.1) — node is a test-host
convenience, never a build dependency. Rejections with reasons live in §7 and stay there
so they are not re-litigated.

### 2.9 Seams and doubles

| Seam | Real | Double | Fault menu |
|---|---|---|---|
| `CameraBackend`/`Camera` (T1/T2) | `webcam-handler-v4l2` | `webcam-handler-fake` (profile replay + scripted faults) | device-gone mid-stream, EBUSY, clamp-on-write [PF:6], INACTIVE flips [PF:3], settle-never-converges, frame timeout, hotplug add/remove |
| Session store (D9) | XDG state dir | temp-dir store | full disk, lock held, torn `log.ndjson` line, foreign `schema_version` |
| Clock/settle | real time | stepped clock | deadline expiry during settle |
| RPC transport | UDS/WS | in-memory jsonrpsee | disconnect mid-subscription |

Fault menus are exhaustive-match-walked enums (rubric A-principles carry from the
predecessor: a fault the compiler cannot force the fake to script is a fault nobody
tests). The fake's frame synthesis responds to control values (§2.3) so calibration tests
assert real behavior (sharpness metric peaks where the fake's focus model peaks — both
directions: an out-of-focus setting must score lower).

### 2.10 Single-copy homes

| Law | Home |
|---|---|
| Control model semantics (types, flags, sparse menus, the slug transform) | `webcam-handler-schema::control` |
| The backend contract (T1/T2 traits, `BackendKind` vocabulary) | `webcam-handler-schema::backend` |
| Auto/manual pairing (declared + measured merge) | `webcam-handler-schema` data + `webcam-handler-engine::pairing` planner |
| Requested-vs-applied write semantics | `Camera::set` contract (T2); engine trusts, never re-reads |
| Atomic state writes | `webcam-handler-engine::store::write_json_atomic` |
| Settle policy | `webcam-handler-engine::settle` |
| Sweep value derivation | `webcam-handler-engine::sweep::plan` |
| Error registry + RPC code mapping | `webcam-handler-schema::error` + `webcam-handler-api::codes` (one match, exhaustive) |
| Command surface (verbs, flags, rendering) | the T4 command core (`webcam-handler-cli-core`), consumed by both CLIs |
| Wire surface | the T5 trait in `webcam-handler-api` |
| Capture settle defaults, size caps, path layouts | `webcam-handler-schema::limits` |
| JPEG pass-through vs re-encode decision (E6) | `webcam-handler-imaging::photo` (pure, bytes→bytes); the engine owns the file sinks that call it |
| The unsafe surface | `crates/backends/v4l2/src/sys/` — the one module allowed to say `unsafe` (§2.5) |

A second copy of any of these is a review finding (docs/8 B-checklist); the gates enforce
the mechanically checkable ones (docs/9).

### 2.11 The backend playbook

Adding a backend (libcamera-compatible devices if the license landscape changes, a remote
proxy, a vendor SDK):

1. New crate `crates/backends/<name>/` implementing T1/T2 against `webcam-handler-schema`
   values only. No engine edits, and the claim is structural: the engine consumes
   `Box<dyn CameraBackend>` and depends on no backend crate (§2.8's edge list).
2. Capture device profiles (T3) from real hardware; commit them; the fake replays them —
   the backend's quirks become corpus the day they are discovered.
3. Add the variant to `BackendKind` (the closed vocabulary in `webcam-handler-schema`)
   and the crate to the two composition roots' factory matches (`wch` and `wchd` each
   hold one exhaustive `match BackendKind` constructing the backend they link) — the
   compiler stops both builds until the new backend is wired, which is the
   compile-fail-on-new-backend property, living where the dependency edges already are.
4. Run the backend conformance battery (`webcam-handler-testkit`): enumeration sanity, control-model
   invariants (D2: unknown types survive round-trip; sparse menus preserved), write
   read-back (D3), snapshot/restore inverse (D4), stream lifecycle, hotplug watch
   lifecycle. The battery is the definition of done; a backend that cannot pass an arm
   carries a named skip with a written reason, counted, never silent.
5. Real-hardware suites are `#[ignore]`d, recipe-named, and leave the camera as found
   (§3.3, §5).

### 2.12 The evidence doctrine

Carried from the predecessor project's E1–E7 where they transfer, re-grounded on
hardware. The numbers moved; the map for dual-series readers: predecessor E1→E1; E2 and
E4 here are new (device authority; PF:6's read-back); predecessor E6 ("an answer is
evidence")→E3; predecessor E7 ("a stand-in resembles")→E5; E6 here restates predecessor
E4/A4 (byte fidelity) in this domain's terms.

- **E1 — Documentation nominates; the device legislates.** The UVC spec, the kernel docs,
  and the crate docs all *nominate*; QUERY_EXT_CTRL *legislates* [PF:1–PF:6]. Transcribed
  device behavior (the D3 pairing table) is data marked `declared` until a probe upgrades
  it to `measured`, and measured wins conflicts.
- **E2 — The device is the only authority on itself.** Capabilities are enumerated at
  open, every time. Committed profiles (T3) are corpus for offline tests and *hints* for
  UX (e.g., last-known controls while a camera is unplugged) — always labeled with capture
  provenance, never silently substituted for a live read.
- **E3 — An answer is evidence; a busy device is not an absent capability.** EBUSY,
  ENODEV, a settle timeout, and a permission refusal each read exactly like "camera can't
  do X" to a lazy caller. The error vocabulary (D13) keeps availability, permission, and
  capability distinct; tests and calibration logic never convert one into another.
- **E4 — Requested is not applied.** [PF:6] Every write's result carries both; every layer
  above (engine, RPC, CLI, session records) preserves both.
- **E5 — A stand-in resembles what it imitates.** The fake backend replays *captured*
  profiles, and its behavioral model (clamping, INACTIVE coupling) is asserted against the
  probe record it claims to reproduce; a fake drifting from its profile is a test failure
  in the fake, not a green run. The doctrine has now paid once in the other direction
  [notes, E4]: the fake refused the `Bytes`-at-a-scalar write that the real backend
  mis-dispatched (§2.3) — a divergence between stand-in and real is a finding against
  *whichever side is wrong*, and this time that was the real one.
- **E6 — Verbatim bytes when bytes are the product.** A photo saved as `.jpg` from an MJPG
  stream is the camera's own bitstream; calibration samples never pass through a decode/
  re-encode round trip that would smuggle codec artifacts into a sharpness comparison.

### 2.13 The privileged development helper (`wch-priv`)

New in v2; note N8 is the full record and the owner rulings live there. Nothing in the
*product* needs privilege — §1's in-process rule is about the product, and it stands.
Development needs three privileged things, each formerly gated on a human typing a
password: loading `vivid` (the R2 rung had never executed for want of it — notes E1/E2),
cycling `uvcvideo` (a soldered-down laptop camera cannot be unplugged, and P4's hotplug
evidence needs a camera to disappear), and binding the P4 uevent socket
(`NETLINK_KOBJECT_UEVENT`, *predicted* to need `CAP_NET_ADMIN` — a prediction P4 must
verify, §8 item 10). `crates/priv/` builds `wch-priv`, blessed once by `just bless` with
`cap_sys_module,cap_net_admin+ep`. The load-bearing facts:

- **It never ships.** It is a dependency of no product crate (gate-asserted,
  `privileged-helper.sh`), and its `modprobe` subprocess is a *development* dependency in
  the same category as the ffprobe oracle — also the license-correct choice, since the
  in-process alternative (`libkmod`) is LGPL and a process boundary is not a link edge.
- **The security boundary is the file mode, not a capability design.** The owner chose
  the generic `exec` wrapper over a closed verb vocabulary — only a wrapper can put a
  capability inside a *test process* — accepting the stated consequence that
  `wch-priv -- /bin/sh` is a root shell. The blessed copy is mode `0700` in gitignored
  `.wch-bin/` (outside `target/`, which cargo rewrites and file capabilities do not
  survive), and `privileged-helper.sh` re-checks the mode on every `just ci`.
- **It refuses to unload `uvcvideo` while any process holds a `/dev/video*` open.** That
  interlock also bounds what tests may do with it: real-hardware device-loss evidence
  runs with cameras closed (hotplug add/remove), and mid-stream loss stays a scripted
  fake fault (§3.3 item 9).
- **The granted powers are broader than the demonstrated need — deliberately, and
  time-boxed.** The owner's ruling accepts the breadth on this machine for the duration
  of the plan. **The trigger to narrow or delete is G6** (docs/7 carries it as a
  post-plan row): which capabilities were actually spent, whether `exec` ever did more
  than delegate to a test process, whether anything routine still loads modules
  unattended. A broad grant with a named revisit is a different thing from a broad grant
  nobody revisits.

## 3. Test architecture

### 3.1 The rung ladder

The predecessor's cost ladder, with hardware access in place of money. Rungs are named
R0–R3, plus the R1-web browser variant of R1 — a fresh letter, because T1–T3 already name backend contracts (§2.3) and one
identifier naming two things is how citations rot:

- **R0 — pure, hermetic (every push):** schema round-trips, planner/state-machine/metric
  properties, imaging codecs on synthetic fixtures, AVI muxer against committed byte
  expectations, error-mapping exhaustiveness, and Miri over the unsafe-adjacent pure
  units (§2.5 — raw-struct decoding as pure functions over captured bytes). No device,
  no daemon, no clock.
- **R1 — fake-backend integration (every push):** engine + store + calibration flows end
  to end over profile replay; daemon + client over in-memory/UDS transports with
  subscriptions; CLI subprocess tests (both binaries) against the fake backend.
- **R1-web — the browser rung (per push where the host has node; counted named skip
  elsewhere):** a **pinned Playwright suite, Chromium project only** — the Chrome-only
  ruling (§2.7) and the test harness agree by construction. Convention transferred from
  serial-nexus — a sibling project in this documentation lineage that drives serial
  hardware with a web console — whose design paid for it (its first green-to-red run found three
  real defects, only one of them in the browser): the suite is launched as a subprocess
  from a `webcam-handler-daemon` integration-gate test that **self-skips without node —
  node is never a build dependency**; browser and package versions are pinned; traces
  are captured on failure. It asserts the browser half *in the browser* instead of
  assuming it from the API: the control panel renders from live `controls` DTOs (sparse
  menus become correct selects), the MJPEG `<img>` actually paints frames, WS JSON-RPC
  round-trips including reconnect, the calibration view tracks its subscription, and the
  token gate turns anonymous requests away. Runs against the fake backend — no camera
  required, so the rung is deterministic.
- **R2 — kernel-virtual (opportunistic gate):** the `vivid` virtual capture driver
  exercises the real ioctl layer without hardware — and a far larger control-model
  surface than the seed hardware: 77 controls against the Chicony's 18 and the OBSBOT's
  24, 83 formats, 747 size entries, and ten compound-payload `G_EXT_CTRLS` reads against
  the hardware's one [notes, E2]. The rung has run and earned its keep twice: its first
  execution found a defect in a hardware test that four green runs against hardware
  could not contradict, and its P2 write/stream arms drove `S_EXT_CTRLS` and the whole
  mmap sequence through a driver the code had never met [notes, E2/E3]. It runs where
  the module is loadable — `just rung-vivid-managed` loads, runs, and unloads via the
  blessed helper (§2.13) — auto-skips elsewhere **with the skip counted and named** (a
  rung that silently becomes a no-op on every runner is the predecessor's "check that
  cannot fail" defect class), and serializes with R3 in the one-thread
  `exclusive-device` nextest group: V4L2 allows one streamer per node, so two hardware
  rungs at once is a real `EBUSY` and a fake failure.
- **R3 — real hardware (`#[ignore]`d, recipe-named, on demand):** enumeration matches the
  committed profile of the attached device (drift is a finding: either the corpus is
  stale or the kernel changed behavior — both are worth knowing); capture produces
  decodable frames; write/read-back and INACTIVE flips on safe controls; snapshot/restore
  leaves the camera byte-identical in control state (asserted). Assertions are invariants,
  never pixel content (lighting varies) — but PF-class regressions (a panic on
  enumeration) are exactly what this rung exists to catch before users do.

### 3.2 Corpus rules

- `corpus/profiles/` holds device profiles (the T3 format) captured by the tool itself,
  with provenance (`captured_at`, kernel, tool version, capturer). Immutable once
  committed; re-capture replaces wholesale with fresh provenance. File names are
  capture-time human-chosen labels; the provenance block inside binds each file to its
  fingerprint, so filenames carry no identity weight. The three probe-era profiles
  (chicony-rgb, chicony-ir, obsbot-tiny3) seed it; every *device-behavior* PF finding
  that is profile-shaped (PF:1–PF:9, PF:12, PF:13 — the node tables and interface paths
  are pinned by the committed profiles) is representable in — and asserted from — at
  least one committed
  profile (the sparse menu [PF:2] and the out-of-range default [PF:5] are regression
  fixtures, not prose). The rest are not profile-shaped, and the corpus-floor gate
  counts only what is: PF:10 lands in the build docs, PF:11 in the fake's settle model,
  PF:14's diagnosis and PF:15's terminator as regression tests in the backend crate, and
  PF:16's splice fixture as a hand-built JPEG in `imaging` — hand-built because the
  frames that exposed it are camera frames, and camera frames never enter the
  repository (§5).
  The hand-authored minimal-repro profile lives in `crates/testkit/fixtures/`, not here:
  this directory stays uniformly tool-captured.
- `corpus/images/` holds **synthetic fixtures only** — generated patterns with known
  metric orderings. Camera frames never enter the repository (§5); the gate that enforces
  this is a content check, not a convention (docs/9).
- Fixtures enter tests as bytes; a fixture nobody loads is dead corpus (gate: corpus
  floor counts, docs/9).

### 3.3 The structural-gap register (v2, honest)

What this suite cannot reach, named up front — regenerated at this revision, not
accreted (rubric rule 4):

1. **Real-hardware truth is unautomatable in shared CI.** R3 runs only where a camera is
   attached; the corpus mitigates (capture once, replay forever) but a new kernel × new
   device interaction is invisible until someone runs R3 against it.
2. **The fake's physics are a model.** Clamping and INACTIVE coupling are replayed from
   profiles; frame synthesis (focus-blur response) is invented. Calibration *logic* is
   fully tested; calibration *efficacy* on real optics is only demonstrable on R3.
3. **USB bandwidth and multi-camera contention** are not modeled; concurrent-stream
   failures on real hubs will look like driver errors (D13 reports them; nothing predicts
   them). §8 item 5.
4. **`vivid` fidelity is partial**: it implements many but not all UVC-typical behaviors
   (no INACTIVE-coupled AE menus with holes, for instance), so R2 proves the ioctl
   plumbing, not device quirks. Executed since P1 with write and streaming arms since P2
   [notes, E2/E3] — the plumbing it proves is materially wider than at v1, and the claim
   about what its green means is unchanged.
5. **The AVI muxer's player-compatibility claim** rests on ffprobe/mpv oracles in CI plus
   manual spot checks; "every player" is not a testable population. Relatedly, AVI is
   constant-frame-rate and delivery is not (D7): the close-time header rewrite bounds the
   error, and the residual VFR mismatch is accepted and named here.
6. **Privacy canary limits**: the no-camera-frames-in-repo gate detects committed image
   files by content sniffing; a frame embedded in an unrecognized container would pass it
   (recorded, accepted — review covers what the gate cannot).
7. **The browser rung drives Chromium only** (R1-web, by owner ruling §2.7): Firefox and
   Safari behavior is unexercised by any automated rung, deliberately. Rendering
   fidelity beyond DOM/protocol assertions stays a manual spot check.
8. **All hardware evidence is one machine, one kernel, three cameras** [notes, E3]. The
   corpus and the vivid rung stand in for the rest of the world, and neither is a
   substitute: a new kernel × new device interaction is invisible until someone runs R3
   somewhere else. A second host would multiply the evidence more than any new suite.
9. **Mid-stream device loss is fake-only on real hardware.** The helper's interlock
   (§2.13) refuses to unload `uvcvideo` under an open node — deliberately — so real
   cycles run with cameras closed (hotplug add/remove events), and `DeviceGone`
   mid-stream is exercised only as the fake's scripted fault. A camera that dies
   mid-stream on a real kernel is therefore modeled, not measured.

## 4. Phased plan

Lives in **docs/7 (implementation plan v2)** — the P0–P2 closure record and the
remaining phases P3–P6, broken into session-sized milestones, with gates G3–G6 as named,
counted, re-runnable criterion sets consuming this document's decision registry
(`scripts/gates/phase-criteria.tsv` is the mechanical home; one row per criterion).

## 5. Hardware and privacy discipline

The analog of the predecessor's spend rules — the resources here are the camera, the
motors, and the user's privacy:

- **A camera frame may contain a person.** Captured frames never enter the repository
  (§3.2), never appear in logs, and land only in caller-named output paths or the
  session's `photos/` directory. Test captures go to gitignored scratch dirs. The web
  client's preview is served, never stored, and `wchd` records nothing it was not asked
  to record.
- **Leave the camera as found.** Every sweep/guarded operation restores state by default
  (D4); R3 test suites assert restoration. `--keep` is the explicit exception.
- **Motors wear, and the two halves of the rule differ (owner ruling, 2026-08-08).** PTZ
  sweeps are bounded everywhere (per-sweep sample caps in
  `webcam-handler-schema::limits`, with the smaller motion cap). In the *product*, a
  motor never moves as an implicit default: a calibration plan that would move motors
  says so before executing, behind `--allow-motion`. In *testing*, the default is the
  opposite: the R3 rung exercises every control the hardware has, motors included — an
  untested motor is untested code, and the OBSBOT is a PTZ device — with
  `WCH_NO_MOTION=1` as the opt-out for runs where the camera is pointed at someone (a
  named, counted skip), and every motion arm restoring the position it moved. The
  ruling is deliberately cheap to reverse: it lives in `scripts/smoke-hw.sh`'s default
  and this paragraph, nowhere else.
- **Busy devices belong to someone.** EBUSY diagnosis names holders (D13); terminating a
  holder is the distinct explicit `terminate-holder` command (D10), which names both the
  camera and the pid, refuses if the pid no longer holds the device, and is never a
  fallback behavior of anything else.
- **The privacy control is honored.** A camera reporting `Privacy` enabled [PF:12] is
  reported as such; the tool does not attempt workarounds.

## 6. Risks

- **The `v4l` crate is semi-dormant (bus factor 1, last release 2023).** Mitigated: pinned
  version; our exposure is narrow — the crate supplies the `v4l2::{open, ioctl, mmap}`
  wrappers and the bindgen types, and both the control layer (PF:1) and, since P2, the
  whole streaming path are already ours; `v4l2r` is the named migration target behind
  T2; worst case the
  ioctl surface we use is small enough to vendor.
- **bindgen at build time** (libclang + kernel headers) — accepted per the owner's
  build-deps-are-fine ruling [PF:10]; CI images and the README must install them; a
  pure-ioctl no-bindgen fallback (`linuxvideo`, 0BSD) exists if this ever becomes
  untenable.
- **jsonrpsee 0.x churn** (0.24→0.26 all broke API): minor pinned workspace-wide; our UDS
  glue is version-coupled and integration-tested so an upgrade PR fails loudly.
- **Kernel/driver variance dwarfs our test matrix.** The design leans on D2/D3 doctrine
  (represent, read back, never trust declared ranges) precisely because quirks are the
  norm — four of the sixteen PF findings are devices contradicting their own
  declarations, and two more (PF:13, PF:15) are the kernel contradicting the
  assumptions every tutorial makes.
- **The AVI muxer is owned code** — bounded (~300 lines, frozen format), oracle-tested;
  the alternative (a dependency) does not exist to buy.
- **Calibration sweeps move physical hardware**; a crashed sweep can leave a camera
  mis-set. Mitigated: snapshot persists to the session dir *before* the first write, so
  `restore` survives a process crash; R1 tests kill a sweep mid-flight and assert
  recovery (the G3 crash-recovery criterion).
- **rust-embed governance** (solo maintainer, self-hosted repo): reviewed on bump;
  `include_dir` fallback named.
- **A root-equivalent development binary lives in the workspace** (`wch-priv`, §2.13).
  Its boundary is a file mode, not a capability design — accepted and time-boxed by
  owner ruling (N8). The residual risk is the revisit being forgotten, which is why the
  G6 trigger is written into docs/7's post-plan table rather than into anyone's memory.

## 7. Considered and not adopted

Recorded with reasons so they are not re-derived:

- **`nokhwa` as the camera layer.** Its Linux backend is the `v4l` crate, so it validates
  our pick while its abstraction destroys what we need: PTZ mapped to `*_RELATIVE` CIDs
  most UVC hardware lacks, menus flattened to integer ranges discarding item names,
  unknown control types hard-error. Wrong altitude for a control-centric tool; reconsider
  only for cross-platform capture.
- **`v4l2r` as the primary backend now.** Most complete ioctl coverage (including the
  event ioctls we will eventually want) and actively maintained — but 0.0.x with explicit
  API instability, and its high-level layers are codec-oriented. Named fallback, not
  foundation.
- **`udev`/`tokio-udev` for hotplug and identity.** The conventional choice, and it would
  hand us stable `ID_*` properties — but it links LGPL libudev. `kobject-uevent` +
  sysfs covers the need license-cleanly; recorded as the alternative if the constraint is
  ever relaxed.
- **`libcamera` bindings.** LGPL native library *and* an abstraction that hides exactly
  the V4L2 control surface this tool exists to expose.
- **Shelling out to `v4l2-ctl`/`ffmpeg`** (the skill's approach): rejected by the owner's
  in-process requirement; also strictly worse for agents (parsing human-oriented output,
  version drift, no typed errors). `ffmpeg-sidecar` (MIT, spawns a user-supplied binary)
  is the recorded escape hatch shape if transcoding ever becomes a hard requirement —
  external tool, opt-in, never bundled.
- **MP4/MKV as the v1 video container.** MP4 cannot carry MJPEG usefully (no standard
  sample entry with broad player support); MKV `V_MJPEG` works (webm-iterable, MIT) but
  AVI+MJPEG has broader player support for this codec by ecosystem consensus (declared,
  not measured — §3.3 item 5) and a simpler muxer. MKV returns with the L1 H.264 path
  (as MP4 via a pure-Rust muxer) where it belongs.
- **SQLite (`rusqlite`) for session state.** All-permissive (bundled SQLite is public
  domain) and it would replace fd-lock — but session state is tens of KB, human-paced,
  and explicitly wanted inspectable; a binary store solves problems we do not have.
  Named trigger for revisiting: cross-session queries at scale.
- **`redb`/`sled`**: same conclusion, plus MSRV (redb) and dormancy (sled).
- **RON/TOML for session state**: session files must be readable by the web client, jq,
  and analysis scripts — JSON's universality wins; TOML remains the *config* format.
- **A Rust-WASM web client** (leptos/yew/dioxus/sycamore): adds trunk + wasm-bindgen +
  wasm32 toolchain and hundreds of KB of payload to render sliders and an `<img>` tag
  that MJPEG feeds natively. Leptos is the named candidate if the client ever grows
  logic that wants shared Rust types.
- **`actix-web`/`warp`/`poem`/`rouille`/`tiny_http`**: axum won on governance (tokio-rs),
  tower ecosystem, and streaming-body fit; poem is the named fallback; details in the
  research record.
- **`jsonrpc-core` family** (deprecated, points at jsonrpsee), **`jsonrpc-v2`** (no
  subscriptions, beta since 2025), **hand-rolled JSON-RPC over axum** (re-implements the
  80% of jsonrpsee we use; retained as the shape of last resort).
- **`daemonize`** (double-fork is an anti-pattern under systemd; dormant), **`figment`**
  (dormant; plain toml suffices), **`colored`** (MPL — license, not taste), **`fs2`**
  (abandoned; fd-lock/fs4 supersede), **`prettytable-rs`** (abandoned), **`dssim`**
  (AGPL; image-compare is the permissive SSIM if ever needed), **`ndarray`/`statrs`**
  (dead weight until response-curve fitting exists — §8 item 6).

## 8. Open questions

1. **The IJG license question — downgraded to dormant.** The adversarial audit
   established that `image` 0.25's JPEG encoding is its own MIT/Apache code (the research
   initially claimed it pulled the `(MIT OR Apache-2.0) AND IJG` `jpeg-encoder`; it does
   not), so **the entire default stack is allowlist-clean and no IJG decision blocks
   v1**. IJG (the permissive attribution license of libjpeg) would enter only via
   optional fast-C decode (`turbojpeg` over libjpeg-turbo, ~2–4× faster than zune-jpeg)
   if profiling ever demands it; decide then, with this note as the starting point. Until
   then those crates sit on the cargo-deny ban list so IJG cannot arrive by accident.
2. **Audio capture** is blocked on licensing (every maintained path links LGPL alsa-lib).
   Revisit if a pure-Rust ALSA PCM implementation appears, the constraint is relaxed for
   dynamically-linked system audio, or PipeWire's native protocol (a socket protocol, no
   linking) grows a maintained pure-Rust client crate.
3. **UVC H.264 (D7 L1)** waits on hardware that exhibits it (E2).
4. **Control-change events** (`VIDIOC_SUBSCRIBE_EVENT`): ~100 lines over bindings we
   already have; lands when a consumer exists (live web-UI control sync is the likely
   trigger).
5. **Multi-camera USB bandwidth limits**: measure once real multi-stream use exists;
   until then D13 reports driver errors honestly.
6. **Metric growth** (response-curve fitting, SSIM stability scoring): named triggers in
   §7; do not build ahead of a calibration session that needs them.
7. **`wch` auto-forwarding to a running daemon** (instead of refusing when the lock is
   held): decide after real usage shows whether the refusal is friction or a feature.
8. **Session retention/GC**: sessions accumulate photos; a `calibrate gc` with
   age/size policy is sketched but uncommissioned until someone's disk fills.
9. **An MCP server surface.** Agents reach the tool today through the CLIs and the
   JSON-RPC daemon, plus the generated agent usage guide (docs/7 P6); a Model Context
   Protocol adapter over the same T5 trait is deliberately uncommissioned until an agent
   runtime that wants one appears — it would be a fourth consumer of the one wire
   surface, not a redesign.
10. **Does the uevent socket actually need `CAP_NET_ADMIN`?** N8 granted it on an
    unverified prediction — the probe that would have answered it was blocked. P4's
    hotplug milestone measures it on this kernel (bind `NETLINK_KOBJECT_UEVENT`
    unprivileged, record the answer in the notes), and the G6 helper-narrowing (§2.13,
    docs/7) consumes the measurement: if the bind is free, the capability was never
    needed and drops.
