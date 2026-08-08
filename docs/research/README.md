# Research record (2026-08-07)

Raw inputs behind design v1 (docs/1). Not normative — the design distills and sometimes
corrects these; where they disagree, docs/1 §2.8 and §7 are the record of decision.

- `probe-findings.md` — the PF registry as captured during design-phase hardware probing
  (also reproduced in docs/1 §1.2, which is the citable copy).
- `crates-*.json` — per-area crate research (candidates, rejections with reasons,
  recommendations, risks), each independently license-audited against crates.io and
  repository LICENSE files on 2026-08-07. Known corrections established by the audits and
  applied in docs/1: `image` 0.25's JPEG encoder is its own MIT/Apache code (not the
  IJG-licensed `jpeg-encoder`, contra `crates-imaging.json`); `kobject-uevent` 0.2 is a
  uevent *parser* — its netlink-sys dependency is dev-only, so the daemon owns the netlink
  socket (contra `crates-v4l2.json`'s fit note).

Versions and licenses here are pins-at-research-time, not commitments; re-verify on
adoption (cargo-deny enforces the posture either way).
