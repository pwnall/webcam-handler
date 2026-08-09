# webcam-handler — Implementation notes

Case law. Recorded, justified deviations from the doc series land here as numbered
**N-entries**; new hardware behavior lands here as **PF-entries continuing the design's
§1.2 registry**. Reviews do not re-report an entry; they retire one only on empirical
disproof.

Each entry states: what the doc says, what the repo does, why, and what would retire it.

**Doc series versioning (2026-08-08):** docs/6–10 (v2) supersede docs/1–5 (v1, now under
`docs/historical/`; v2 preserves v1's section and registry numbering, so the citations
below still resolve). The v2 revision absorbed the design- and gate-facing halves of the
entries below and PF:13–16 into the current docs; each absorbed entry carries an
**Absorbed:** line naming the new home. Absorption does not retire an entry — these
remain the measurement record and the reasoning of record. Entries dated before this
line cite docs/1–5; later entries cite docs/6–10.

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
(fallback `$HOME/.local/state`) and `$XDG_RUNTIME_DIR` directly.

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
| The P4 uevent socket | Binding `NETLINK_KOBJECT_UEVENT` needs `CAP_NET_ADMIN`. *Unverified* — the probe was blocked — so this capability is granted ahead of proof, which is recorded here rather than discovered later. |

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
   whole design to remove and the easiest to forget.
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
