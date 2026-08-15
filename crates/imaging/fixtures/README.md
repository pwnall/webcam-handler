# Byte fixtures for the imaging crate

Two families. **A frozen AVI**, `avi/two-frame-64x48.avi`, loaded with `include_bytes!` by
`crates/imaging/src/avi/write.rs`; and since P6b **two frozen Y4Ms**, `y4m/two-frame-64x48-c422.y4m`
and `y4m/two-frame-64x48-mono.y4m`, loaded by `crates/imaging/src/y4m.rs`. All three are
**synthetic** — every sample in them was generated, no camera was involved, and the tests that
load them assert exactly that (see "Privacy" below).

These are **not** the device-profile corpus. `corpus/profiles/` holds T3 profiles captured
by `webcam-handler-cli profile capture` and is uniformly tool-captured (design §3.2); these
are byte-level fixtures belonging to one crate's muxers, so they live with them — the same
split `crates/backends/v4l2/fixtures/` makes. One directory per container rather than a shared
`video/`, because the two are separate modules with separate frozen formats and a shared
directory is where a fixture ends up loaded by the module it does not belong to.

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
- `crates/imaging/src/y4m.rs`'s `#[cfg(test)] parse_y4m` — a ~90-line parser derived from the
  YUV4MPEG2 format description, not from `header_bytes` or the plane fills, which re-derives
  the plane sizes from the `C` tag it read. Its own comment lists the eight things it refuses,
  and `the_parser_refuses_the_ways_this_writer_could_be_wrong` asserts them before it is
  trusted with a round trip. The `y4m` crate's decoder would **not** have served: it ships in
  the same file as the encoder it would have been checking (note **N107**).
- `testkit::oracle` — the ffprobe/mpv rung P6d added: a third party that has never read our
  code, handed files our muxers produced. It is what measured the padded `F` denominator these
  two files now carry (note **N106**'s amendment, evidence **E17**), which is the one time an
  oracle has changed what a fixture holds rather than only confirming it.

## Provenance — `avi/two-frame-64x48.avi`

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

## Provenance — `y4m/two-frame-64x48-c422.y4m` and `y4m/two-frame-64x48-mono.y4m`

| Field | Value |
|---|---|
| Generated | 2026-08-15 (re-generated: the header's rate field became fixed-width and is now patched at close — see below) |
| Nature | **Synthetic**. Generated samples, written by this workspace; no camera, no capture |
| Generator | `imaging::y4m::Y4mWriter` over two frames from `imaging::y4m::tests::raw_frame` |
| Samples | Every sample is a function of its own position: `luma(x, y) = (7x + 13y) % 251`, `cb(x, y) = 64 + (3x + 5y) % 61`, `cr(x, y) = 160 + (11x + 2y) % 61`. Three primes, three disjoint value ranges — so a plane shifted by a row, a column, or swapped with its sibling is a plane full of wrong numbers rather than one that happens to match |
| Streams | 64×48 `YUYV` → `C422`, and 64×48 `GREY` → `Cmono` |
| Interval | `negotiated_interval_us = 66_666` (15 fps), written at `begin` as `F1000000:0000066666` and **patched at close** to the measured mean `F1000000:0000050000`. Deliberately **not** the 33 333 µs placeholder, so a sink that dropped the negotiated number could not match these bytes; and deliberately not equal to the mean, so a sink that stopped patching could not either |
| Timestamps | 0 µs and 50 000 µs, sequences 0 and 1 — a measurable mean of 50 000 µs, which since P6d is what the header carries. It used to be what the header pointedly did **not** carry, and the same two timestamps pinned that (note **N106** and its amendment) |
| Caps | `max_bytes = 1 MiB`, `max_frames = 64`, `max_span = 60 s` — none of them reached |
| Byte order | None. Every sample is one byte and every header field is decimal text, which is one of the few things Y4M makes simpler than RIFF |

**Two files and not three**, and the limit is worth stating rather than leaving to be
inferred. The three colorspaces differ in the header's `C` tag and in their plane sizes, and
both of those are derived and asserted for all three by the independent parser, so a third
frozen file would add only the byte-level layout of `C420`. `C422` is frozen because it is the
richest layout — three non-empty planes at 4:2:2 sizes, extracted from a *packed* buffer — and
`Cmono` because it is the opposite shape and the one whose two empty planes a sink could
silently stop writing. `C420` sits between them on both counts and is covered by the round trip
rather than by bytes.

Immutable once committed, like the ioctl replies and the profile corpus: a deliberate format
change replaces the file wholesale and updates the table above with what moved and why.

## What each file pins

| File | Pins |
|---|---|
| `y4m/two-frame-64x48-c422.y4m` | 12 346 bytes: the header line `YUV4MPEG2 W64 H48 F1000000:0000050000 Ip C422`, whose denominator is zero-padded to ten digits so `finish` can patch it in place, and with **no `A` field** because V4L2 does not report a pixel aspect and `A1:1` would be a claim; a six-byte `FRAME\n` with no parameters before each frame; and the three planes in Y, Cb, Cr order at 3072/1536/1536 bytes, de-interleaved from packed `Y0 Cb Y1 Cr` without averaging a sample |
| `y4m/two-frame-64x48-mono.y4m` | 6 203 bytes: the same header shape with `Cmono`, and one 3072-byte luma plane per frame followed by **nothing** — the two empty chroma planes a `Cmono` reader must not be offered |
| `avi/two-frame-64x48.avi` | 1852 bytes of finished AVI: the 224-byte header list complete before the first frame, `avih`/`strh`/`strf` at the offsets `finish` seeks to, an odd frame followed by a zero pad byte that its own size field and its `idx1` entry both exclude, an `idx1` of two `AVIIF_KEYFRAME` entries at `movi`-relative offsets 4 and 422, `AVIF_HASINDEX` set, and the **measured** 50 000 µs interval in both `avih.dwMicroSecPerFrame` and `strh.dwScale` (D7's CFR carve-out, close-time rewrite) |

## Privacy

Design §5 and AGENTS' "Hardware and privacy": a frame may contain a person, and camera
frames never enter this repository. These files are generated, and the assertion that says so
is in the tests rather than in this paragraph —
`the_frozen_fixture_is_still_the_file_the_muxer_emits` re-generates both JPEGs from
`imaging::fixtures`, and `the_frozen_fixtures_are_still_the_files_the_y4m_sink_emits`
re-generates every sample from `luma`/`cb`/`cr`, and each compares the result against what it
reads back out of the committed bytes. If a camera frame were ever spliced in here, those
comparisons would fail.

**The Y4M half is the one where a leak would be readable with `od`.** An AVI hides its frames
inside JPEG bitstreams; a Y4M payload *is* the picture, one 8-bit luma sample per byte in
raster order. So the same three conditions apply to it and the gate reads its `W`/`H` out of
the header line.

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

**P6b found the same hole one container along and closed it the same way.** A `YUV4MPEG2 `
header matched no magic number either, so both Y4M fixtures reported as "not an image" and the
run stayed green over two files of raw luma. The gate now sniffs the ten-byte magic, reads the
extent out of the header line, gives Y4M its own home directory, and five `fail_case_y4m_*`
arms — outside its directory, unprovenanced, in the *AVI* directory, over the extent cap, and
with an unreadable extent — prove that half can go red. That two consecutive containers
arrived invisible is the argument for the format list being a decision somebody makes rather
than a thing that grows on its own.

Design §3.3 item 6's honest limit still stands one container along — "a frame inside an
unrecognised container passes, and review carries that half" — and the regeneration
assertion is what says these particular pixels were generated rather than merely that they
are small and declared.

## Regenerating

There is no blessing recipe, because each fixture is one call away from the test that reads
it: `fixture_params()` and `fixture_frames()` in `crates/imaging/src/avi/write.rs` and in
`crates/imaging/src/y4m.rs` are the whole input, and the tables above restate them so a file
can be rebuilt without reading the code. To regenerate deliberately, run the recording those
functions describe and write the resulting bytes over the file — then say in the commit what
changed in the format and why, because a fixture that moves without an explanation is a format
that drifted.
