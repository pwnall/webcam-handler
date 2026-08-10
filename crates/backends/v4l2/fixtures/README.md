# Byte fixtures for the V4L2 backend

Two families, captured the same way and loaded the same way: **ioctl replies** (P1/P2,
`sys::decode`) and **uevent netlink datagrams** (P4d, `hotplug`). Each file is bytes a real
kernel produced, verbatim, with one deliberate exception — the `uevent-hostile-*` files,
which are synthetic because no kernel produces them and which say so below.

## Raw ioctl reply fixtures

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

---

## Uevent netlink datagrams

Each `uevent-*.bin` is **one `NETLINK_KOBJECT_UEVENT` group-1 datagram, verbatim** — the
bytes the kernel broadcast, copied out unchanged — except the `uevent-hostile-*` family,
which is synthetic (see below). They exist so `hotplug::trigger` is a pure function over
bytes a real kernel produced, which is what makes the hostile-bytes claim testable at all:
rubric B10 says a packet off a kernel socket is attacker-shaped input, and an assertion
about attacker-shaped input that only a driver cycle can reach is an assertion nobody runs.

### Provenance

| Field | Value |
|---|---|
| Captured | 2026-08-10 |
| Kernel | 7.0.0-29-generic (x86_64) |
| Driver | `uvcvideo` |
| Cameras | Chicony `3-4:1.0` RGB and `3-4:1.2` IR; OBSBOT Tiny 3 `3-1:1.0`; Dell U3224KB/A `2-3.4.1.1:1.0` \[PF:19\] — ten `video4linux` nodes, four cameras |
| Trigger | one `.wch-bin/wch-priv uvcvideo cycle`, every camera closed, no `--force` |
| Listener | uid 1000, `CapEff: 0000000000000000` — **no capability at all** \[PF:21\] |
| Socket | `socket(AF_NETLINK, SOCK_DGRAM\|SOCK_CLOEXEC, NETLINK_KOBJECT_UEVENT)`, `bind(nl_pid=0, nl_groups=1)` |
| Method | an independent Python socket, **not** this crate's `sys::uevent` — a fixture produced by the code under test proves nothing |

Immutable once committed, like the ioctl replies above: re-capture replaces a file wholesale
and updates this table. These carry no host byte order — a uevent is NUL-separated ASCII —
with one exception noted in the table below.

### What each file pins

| File | Pins |
|---|---|
| `uevent-add-video-node.bin` | the shape, verbatim: `add@…` header, then `ACTION`/`DEVPATH`/`SUBSYSTEM`/`MAJOR=81`/`MINOR`/`DEVNAME`/`SEQNUM`. Two things `kobject-uevent`'s own committed fixture does not have — a **trailing NUL**, and `DEVNAME=video0` as a bare name with no `/dev/` prefix |
| `uevent-remove-video-node.bin` | the `remove` counterpart, on the Dell's longest `DEVPATH` \[PF:19\] |
| `uevent-bind-usb-interface.bin` | another subsystem **and** another verb at once: `SUBSYSTEM=usb`, `ACTION=bind`. The filter must drop it, and it must not be an error |
| `uevent-add-media-node.bin` | the nearest neighbour — the *same camera's* `media0`, from the same burst, so the filter is shown dropping a packet from the very device it is watching. Its `DEVPATH` is `…/3-4:1.0/media0` and contains no `video4linux`, so it does **not** discriminate the `SUBSYSTEM` filter from a path substring; nothing this desk broadcasts does, and the synthetic packet that does lives in `hotplug`'s `a_neighbours_packet_under_a_video4linux_path_is_still_not_ours` |
| `uevent-cycle-burst.bin` | one whole `uvcvideo` cycle: 56 datagrams, of which 10 are `video4linux` adds, 10 are removes and 36 belong to `usb`/`media`/`module`/`drivers`. **Framing is ours, not the kernel's** — each datagram sits behind its own little-endian `u32` length, because a file carries no datagram boundaries and the plain concatenation decodes as *one* packet (`hotplug`'s header measured that). Also the fuzz corpus |
| `uevent-hostile-no-nul.bin` | **synthetic** — every NUL removed, so the packet has no separators and no terminator |
| `uevent-hostile-truncated-header.bin` | **synthetic** — the header line cut mid-path at 12 bytes, and nothing after it |
| `uevent-hostile-key-without-equals.bin` | **synthetic** — `SUBSYSTEM=video4linux` with the `=` turned into a `_`, so the segment is skipped and the key is absent |
| `uevent-hostile-absurd-numbers.bin` | **synthetic** — `MAJOR`, `MINOR` and `SEQNUM` all absurd, the last one past `u64::MAX`. The uevent format has no length field, so this is the only number in it a sender can lie about; the *declared length* a socket can lie about is `MSG_TRUNC`'s, which `sys::uevent` checks |
| `uevent-hostile-embedded-nuls.bin` | **synthetic** — a run of 64 NULs spliced between two fields and 32 more after the last. Not a refusal: empty segments are skipped and the packet still decodes, which is the assertion |
| `uevent-hostile-not-utf8.bin` | **synthetic** — four bytes no UTF-8 decoder accepts, inside a field this build never reads. `kobject-uevent` validates the whole buffer, so the packet is refused entirely |
| `uevent-hostile-enormous.bin` | **synthetic** — 16 KiB, twice `limits::UEVENT_PACKET_BYTES`, padded with distinct keys so the parser's map grows with the packet. A large *well-formed* packet at the parser, and an `Oversized` refusal at the socket |

The `uevent-hostile-*` files are the one place this directory's "bytes a real kernel
produced" rule is deliberately broken, and it cannot be otherwise: a kernel does not
broadcast a truncated datagram (netlink delivers one atomically or not at all), so every
hostile shape here is a named mutation of `uevent-add-video-node.bin` rather than a
capture. Each mutation is spelled out in the table above and reproduced by
`hotplug`'s own tests, which assert a *different* answer for each — "malformed" and "a
shape this build cannot read" must not be the same answer (AGENTS rule 6).
