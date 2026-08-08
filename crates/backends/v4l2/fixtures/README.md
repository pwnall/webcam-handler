# Raw ioctl reply fixtures

Each file is **one V4L2 ioctl reply, verbatim** — the bytes the kernel wrote into the
argument struct, copied out unchanged. They exist so `sys::decode`'s units are pure
functions over bytes a real kernel produced rather than over bytes a test author typed,
which is what makes the Miri job (`just miri`, design §2.5) worth running: Miri cannot
cross an ioctl, so the decoding half has to be reachable without one.

These are **not** the device-profile corpus. `corpus/profiles/` holds T3 profiles captured
by `wch profile capture` and is uniformly tool-captured (§3.2); these are sub-profile
byte-level fixtures belonging to one crate's decoder, so they live with it.

## Provenance

| Field | Value |
|---|---|
| Captured | 2026-08-08 |
| Kernel | 7.0.0-29-generic (x86_64) |
| Driver | `uvcvideo` |
| Cameras | Chicony `04f2:b83c` (video0 RGB, video3 IR metadata node); OBSBOT Tiny 3 `3564:ff02` (video4) |
| Method | direct `ioctl(2)` from an independent probe, not from this crate — a fixture produced by the code under test proves nothing |

Immutable once committed, like the profile corpus: re-capture replaces a file wholesale
and updates this table. Byte order is the capturing host's (x86_64 little-endian), which
is also the only order these bytes are ever read in — they never leave the machine that
produced them in production, and `sys::fields` reads them with `from_ne_bytes` to say so.

## What each file pins

| File | Pins |
|---|---|
| `querycap-chicony-rgb.bin` | a capture node's `device_caps` (`0x04200001`) differing from the device-wide `capabilities` (`0x84a00001`) |
| `querycap-chicony-ir-metadata-node.bin` | a **metadata** node — same `bus_info` as the RGB camera \[PF:13\], `device_caps` `0x04a00000` with no `VIDEO_CAPTURE` bit \[PF:7\] |
| `querycap-obsbot-tiny3.bin` | a card name truncated into the kernel's 32-byte field |
| `query_ext_ctrl-chicony-roi-rect.bin` | **PF:1** — control type `263` (`0x0107`, `RECT`), `elem_size` 16, `HAS_PAYLOAD`. The type that panics `v4l::query_controls` |
| `query_ext_ctrl-chicony-auto-exposure.bin` | the menu control whose indices have holes \[PF:2\] |
| `querymenu-chicony-auto-exposure-{1,3}.bin` | **PF:2** — the two indices that exist. 0 and 2 return `EINVAL` and so have no file, which is the finding |
| `query_ext_ctrl-chicony-white-balance-temperature.bin` | **PF:3** — flags `0x1010`: `INACTIVE` set because automation held it at capture time. Also a non-unit step (10) |
| `query_ext_ctrl-chicony-privacy.bin` | **PF:12** — `READ_ONLY` (`0x4`), and a control whose flag word carries *no* `0x1000` bit |
| `query_ext_ctrl-obsbot-zoom-continuous.bin` | **PF:4**'s control (range `[-100..100]`; the out-of-range current `245` is a `G_EXT_CTRLS` fact, not a `QUERY_EXT_CTRL` one) |
| `query_ext_ctrl-obsbot-power-line-frequency.bin` | **PF:5** — declared range `[0..2]`, declared default `3` |
| `query_ext_ctrl-obsbot-pan-absolute.bin` | a wide range (`±468000`) at step `3600` — the sweep-cap motivation in `limits::MAX_MOTION_SWEEP_SAMPLES` |
| `enum_fmt-chicony-mjpg.bin` | a compressed format's `flags` bit and its FourCC word |
| `enum_framesizes-chicony-mjpg-0.bin` | a discrete frame size |
| `enum_frameintervals-chicony-mjpg-0-0.bin` | a discrete frame interval (1/30) |
