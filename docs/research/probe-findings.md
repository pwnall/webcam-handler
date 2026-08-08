# Probe findings (PF registry) — measured 2026-08-07 on kernel 7.0.0-29-generic

Hardware: Chicony Integrated Camera (04f2:b83c, RGB + IR logical cameras) and OBSBOT
Tiny 3 (PTZ). Probes written in Rust against the `v4l` 0.14.0 crate + raw ioctls,
built on rustc 1.97.1, edition 2024.

- **PF1 — The `v4l` crate panics on modern control types.** `query_controls()` unwraps
  `Type::try_from` which lacks 0x0107 (RECT); the Chicony exposes `Region of Interest
  Rectangle` (0x00981ae1, type rect, elem_size=16) on this kernel, so enumeration
  panics (v4l-0.14.0/src/control.rs:172). `MenuItem::try_from` has the same shape.
  Consequence: own the control-enumeration layer via raw VIDIOC_QUERY_EXT_CTRL
  (NEXT_CTRL|NEXT_COMPOUND), with unknown types represented, never fatal.
- **PF2 — Menu indices are sparse.** Chicony `Auto Exposure` has items {1,3};
  OBSBOT has {0,1,3}. VIDIOC_QUERYMENU fails (EINVAL) on holes; enumeration loops
  min..=max tolerating failures. Item names differ per device ("Manual Mode" is index 1
  on both here, but discovery must read names, not assume indices).
- **PF3 — INACTIVE tracks auto/manual pairing live, both directions.** Setting OBSBOT
  `white_balance_automatic=1` flips `white_balance_temperature` flags 0x1000→0x1010
  (INACTIVE=0x0010 set); back to 0 clears it. Pairing is empirically discoverable by
  toggling the auto control and diffing INACTIVE across the control set.
- **PF4 — Current values can sit outside the declared range.** OBSBOT
  `Zoom, Continuous`: range [-100..100], current=245. Readers must not validate
  current-value against range; report as measured.
- **PF5 — Defaults can sit outside the declared range.** OBSBOT `Power Line Frequency`:
  menu range [0..2], default=3.
- **PF6 — Out-of-range writes are silently clamped, not refused.** S_CTRL
  white_balance_temperature=99999 (max 10000) returns success; driver applies 10000.
  Every set must read back and report requested vs applied. (Spec says drivers MAY
  ERANGE; uvcvideo clamps.)
- **PF7 — One USB device can host multiple logical cameras; grouping is by USB
  interface.** Chicony RGB = 3-4:1.0 (video0 capture + video1 metadata), Chicony IR =
  3-4:1.2 (video2 GREY capture + video3 metadata), OBSBOT = 3-1:1.0 (video4+5). Media
  controller devices (/dev/media0..2) mirror the grouping 1:1. Capture vs metadata
  nodes distinguished by device_caps: VIDEO_CAPTURE vs META_CAPTURE — never by node
  numbering convention.
- **PF8 — Serial numbers are unreliable identity.** Chicony reports serial "0001";
  OBSBOT reports none at the interface parent. Stable identity cannot assume serials.
- **PF9 — In-process capture validated.** mmap streaming via the `v4l` crate: OBSBOT
  1920x1080 MJPG frame (valid SOI/EOI JPEG, ~150KB) in 2.0s including 10-frame settle;
  Chicony 0.48s. OBSBOT MJPG offers up to 3840x2160; YUYV capped at 640x480 on both
  (USB bandwidth) — frame-size lists are per-pixel-format.
- **PF10 — Build deps.** `v4l` 0.14 + `v4l2-sys-mit` = pure-ioctl (no libv4l runtime
  dep) but bindgen at build time (libclang). Compiles clean on edition 2024.
- **PF11 — Early frames are unsettled.** First frames after STREAMON are dark/blue
  before AE/AWB converge; skip-N-frames (or a settle deadline) is required for photos.
- **PF12 — Read-only controls exist; the decoded flag set must expect growth.** Chicony
  `Privacy` is READ_ONLY (0x0004). Most int controls carry flag bit 0x1000 on this
  kernel — identified after the initial capture as `V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX`
  (recent kernels set it widely; it arrived with the same kernel work as the RECT/ROI
  support behind PF1). Represent flags as raw bits + decoded known set. (The original
  capture called the bit "undocumented"; docs/1 §1.2 carries the corrected wording.)

Devices probed (full dumps in scratchpad shell history):
- Chicony RGB: MJPG 320x180..2592x1944@30/15, YUYV ≤640x480; 18 controls incl. ROI
  rect + ROI auto bitmask; auto_exposure menu {1: Manual, 3: Aperture Priority}.
- Chicony IR: GREY 8-bit (640x360@15 typical); separate logical camera.
- OBSBOT Tiny 3: MJPG 1920x1080/3840x2160/1280x720/1280x960/1920x1440, YUYV 640x360,
  640x480; PTZ: pan/tilt absolute ±468000/±324000 step 3600 (arc-seconds), zoom
  0..100, focus 0..100 + focus_automatic_continuous, pan/tilt speed controls,
  zoom_continuous; auto_exposure menu {0: Auto, 1: Manual, 3: Aperture Priority}.
