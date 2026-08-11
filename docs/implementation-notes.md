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

**What is left of this entry.** Only the daemon-status half: the actor registry is still
`engine::actor::Cameras::activity` and still not on the wire.

**Retires when:** a daemon status reaches the wire — which is a T5 method, so a docs/7
sub-milestone has to want one.

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
