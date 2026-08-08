# webcam-handler — Implementation notes

Case law. Recorded, justified deviations from docs/1–5 land here as numbered **N-entries**;
new hardware behavior lands here as **PF-entries continuing the docs/1 §1.2 registry**.
Reviews do not re-report an entry; they retire one only on empirical disproof.

Each entry states: what the doc says, what the repo does, why, and what would retire it.

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

---

## N3 — `std::thread::sleep` is banned workspace-wide, not just in tests

**Doc:** docs/4's `disallowed-methods` bans `std::thread::sleep` *in test code*.

**Repo:** `clippy.toml` is a workspace-global file with no notion of test-vs-not, so the
ban is global. Legitimate production sites (there are none yet; a settle backoff would be
one) take a narrow `#[expect(clippy::disallowed_methods, reason = "…")]`.

**Why:** the same one-home argument as N1. A global ban with named exceptions is auditable
— `grep` finds every exception and each carries a reason; a test-only ban is not.

**Retires when:** clippy grows per-target `disallowed-methods`.

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
