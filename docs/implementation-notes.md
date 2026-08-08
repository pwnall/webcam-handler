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
