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
