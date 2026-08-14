# webcam-handler

Drive V4L2 webcams from a terminal, a daemon, a browser, or an AI agent: enumerate
cameras, read their real capabilities, move their controls, take photos and videos, and
run calibration sessions that record what worked.

Everything happens in-process. No `v4l2-ctl`, no `ffmpeg`, no external binaries at
runtime — the tool links Rust libraries and talks to the kernel itself.

**Status: under construction.** The architecture is settled (`docs/6`), the work is
phase-gated (`docs/7`; P0–P2 are closed), and this README grows a usage section when
`webcam-handler-cli` grows verbs.

## Deliverables

| | |
|---|---|
| `webcam-handler-cli` | the direct CLI — drives the engine in-process |
| `webcam-handler-daemon` | the daemon — JSON-RPC over a Unix socket always, opt-in loopback TCP for the browser |
| `webcam-handler-client` | the daemon CLI client — the same command surface as `webcam-handler-cli`, over the wire |
| web client | vanilla JS served by `webcam-handler-daemon`: camera list, live preview, controls, calibration |
| the library | `webcam-handler-engine` over a pluggable `CameraBackend` trait |

## Building

Rust 1.97 or newer, edition 2024. The V4L2 backend runs `bindgen` at build time, so the
build host needs libclang and the kernel UAPI headers:

```sh
sudo apt install clang libclang-dev linux-libc-dev   # Debian/Ubuntu
cargo build --locked --workspace
```

Nothing external is needed at *runtime*. `ffprobe` and `mpv` appear only as test oracles,
and `node` only for the browser test rung — neither is ever a build dependency.

## Using a camera

Your user needs read/write access to the `/dev/video*` nodes. On a desktop system logind
grants it to the seat owner automatically; otherwise:

```sh
sudo usermod -aG video "$USER"   # log out and back in
```

`webcam-handler-cli` says so itself, with the path it could not open, rather than failing
obscurely.

## Development

```sh
just ci          # everything CI runs, offline: fmt, clippy, tests, docs, licenses, gates
just gate-g0     # a phase gate — named, counted, re-runnable
just selftest    # proves every gate predicate can go red, in both directions
just smoke-hw    # the real-hardware rung; needs a camera, moves no motors by default
just rung-web    # the browser rung, in a pinned headless Chromium; declines by name without
                 # node — `just rung-web-install` is what supplies it
```

Read `AGENTS.md` before changing anything. The reasoning behind every rule it states lives
in `docs/`.

## License

MIT or Apache-2.0, at your option. Every dependency is permissively licensed, enforced by
`cargo-deny` (`deny.toml`) rather than by good intentions.
