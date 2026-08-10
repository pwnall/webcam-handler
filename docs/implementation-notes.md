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

One row remains, and it still names its sub-milestone: `api::codes::typed`'s client-side
consumer is P4f's `wchc`.

**Retires when:** P4f lands its call site. Each declaration names which, so the review that
closes G4 can check them off rather than rediscover them.

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

**Retires when:** P4e lands the orderly exit. Unlinking on the way out is still not a
substitute for this — a daemon that is killed never runs its exit path — so the rule stays;
what changes is that the leftover becomes the exception rather than the rule.

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

**Retires when:** a daemon status reaches the wire — which is a T5 method, so a docs/7
sub-milestone has to want one — or **P4e** lands the `wait` flag with the enqueue that
honours it.

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

**Retires when:** P4e bounds an actor command; this entry then records the layered answer
rather than a whole one.

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
