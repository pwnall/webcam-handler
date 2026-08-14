# Byte fixtures for the imaging crate

One family so far: **a frozen AVI**, `avi/two-frame-64x48.avi`, loaded with `include_bytes!`
by `crates/imaging/src/avi/write.rs`. It is **synthetic** — every pixel in it was generated
by `imaging::fixtures`, no camera was involved, and the test that loads it asserts exactly
that (see "Privacy" below).

These are **not** the device-profile corpus. `corpus/profiles/` holds T3 profiles captured
by `webcam-handler-cli profile capture` and is uniformly tool-captured (design §3.2); this
is a byte-level fixture belonging to one crate's muxer, so it lives with it — the same split
`crates/backends/v4l2/fixtures/` makes.

## What this fixture proves, and what it does not

It pins a **frozen format against silent drift**. Nothing else. A field reordered, a flag
set at `begin` instead of at `finish`, a `dwQuality` that grew a value, a pad byte that
moved into a size field — each of those changes these bytes, and the test that compares them
is the only thing in the suite that notices a format change no other assertion happens to
look at.

**A fixture produced by the code under test proves nothing about correctness.** It cannot:
if the muxer were wrong, this file would be wrong in exactly the same way and the comparison
would still be green. Two other things carry correctness, and they are the ones to read
first:

- `crates/imaging/src/avi/read.rs` — an AVI reader written from the RIFF/AVI specification
  **before** the muxer existed, sharing no constant with it, which refuses every size field
  that disagrees with the bytes present (docs/7 P6a's "independent re-parse path that is not
  the writer's code").
- The ffprobe/mpv oracle rung P6d adds — a third party that has never read our code, over
  files our muxer produced.

## Provenance

| Field | Value |
|---|---|
| Generated | 2026-08-14 |
| Nature | **Synthetic**. Generated pixels, encoded by this workspace; no camera, no capture |
| Generator | `imaging::avi::write::AviWriter` over two JPEGs from `imaging::fixtures` |
| Frame 0 | `encode::jpeg(&Decoded::Gray(fixtures::checkerboard(64, 48, 8)), 15)` — 409 bytes, **odd**, so the RIFF pad byte is in the frozen bytes |
| Frame 1 | `encode::jpeg(&Decoded::Gray(fixtures::text_like(64, 48)), 30)` — 1162 bytes, even |
| Stream | 64×48, `MJPG`, `negotiated_interval_us = 33_333` (30 fps) |
| Timestamps | 0 µs and 50 000 µs, sequences 0 and 1 — so the close-time rewrite has something to do, and the file declares the **measured** 50 000 µs rather than the negotiated 33 333 |
| Caps | `max_bytes = 1 MiB`, `max_frames = 64`, `max_span = 60 s` — none of them reached |
| Byte order | Little-endian, which is the format's, not the host's: RIFF is little-endian everywhere, and the muxer writes `to_le_bytes` on every field |

Immutable once committed, like the ioctl replies and the profile corpus: a deliberate format
change replaces the file wholesale and updates this table with what moved and why.

## What each file pins

| File | Pins |
|---|---|
| `avi/two-frame-64x48.avi` | 1852 bytes of finished AVI: the 224-byte header list complete before the first frame, `avih`/`strh`/`strf` at the offsets `finish` seeks to, an odd frame followed by a zero pad byte that its own size field and its `idx1` entry both exclude, an `idx1` of two `AVIIF_KEYFRAME` entries at `movi`-relative offsets 4 and 422, `AVIF_HASINDEX` set, and the **measured** 50 000 µs interval in both `avih.dwMicroSecPerFrame` and `strh.dwScale` (D7's CFR carve-out, close-time rewrite) |

## Privacy

Design §5 and AGENTS' "Hardware and privacy": a frame may contain a person, and camera
frames never enter this repository. This file is generated, and the assertion that says so
is in the test rather than in this paragraph —
`the_frozen_fixture_is_still_the_file_the_muxer_emits` re-generates both JPEGs from
`imaging::fixtures` and compares them against the frames it reads back out of the committed
bytes. If a camera frame were ever spliced in here, that comparison would fail.

That assertion is not the only thing standing behind it, and until the P6a review it was.
`scripts/gates/no-frame-bytes-in-repo.sh` sniffed image magic numbers, and a `RIFF`/`AVI `
container is not one of them — this file reported as "not an image", the run stayed green,
and a second AVI committed anywhere tomorrow would have been invisible to every predicate in
the suite while the in-crate assertion went on covering exactly one file by name. The gate
now walks an AVI to its `avih` and holds it to the same three conditions the still fixtures
get: **this** directory is the only place an AVI may live, it must carry a `generated-by`
marker (here `two-frame-64x48.avi.provenance.toml`, a sidecar because our muxer writes no
comment chunk and giving it one would move the bytes this fixture freezes), and its declared
frame extent must be under the same sub-VGA cap. Four `fail_case_avi_*` arms in
`scripts/gates/cases/no-frame-bytes-in-repo.cases.sh` prove that half can go red.

Design §3.3 item 6's honest limit still stands one container along — "a frame inside an
unrecognised container passes, and review carries that half" — and the regeneration
assertion is what says these particular pixels were generated rather than merely that they
are small and declared.

## Regenerating

There is no blessing recipe, because the fixture is one call away from the test that reads
it: `fixture_params()` and `fixture_frames()` in `crates/imaging/src/avi/write.rs` are the
whole input, and the table above restates them so the file can be rebuilt without reading
the code. To regenerate deliberately, run the recording those two functions describe and
write the resulting bytes to `avi/two-frame-64x48.avi` — then say in the commit what changed
in the format and why, because a fixture that moves without an explanation is a format that
drifted.
