# webcam-handler

Drive V4L2 webcams from a terminal, a daemon, a browser, or an AI agent: enumerate
cameras, read their real capabilities, move their controls, take photos and videos, and
run calibration sessions that record what worked.

Everything happens in-process. No `v4l2-ctl`, no `ffmpeg`, no external binaries at
runtime — the tool links Rust libraries and talks to the kernel itself.

**Status: under construction.** The architecture is settled (`docs/6`) and the work is
phase-gated (`docs/7`): P0–P4 are closed, P5 (the web client) is closing, and P6 (video
recording) is not written yet. Everything below works today; `record` is what is missing.

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

## Installing

Each binary installs from its own crate. Install the two you need, or all four.

```sh
cargo install --locked --path crates/daemon    # webcam-handler-daemon  — the daemon
cargo install --locked --path crates/client    # webcam-handler-client  — the daemon's CLI client
cargo install --locked --path crates/cli       # webcam-handler-cli     — the direct, daemon-less CLI
cargo install --locked --path crates/priv      # webcam-handler-priv    — dev-only; see AGENTS.md
```

They land in `~/.cargo/bin`. `--locked` is not decoration: it installs the dependency
versions this project tests against rather than whatever resolves today.

The binary is named after its crate, so the directory and the program differ on the same
line — `crates/daemon` builds `webcam-handler-daemon`. That is deliberate (note **N90**).

**`webcam-handler-priv` is dev-only and root-equivalent.** It exists to load kernel modules
for one test rung, it is not part of using a camera, and it does nothing until it is
deliberately granted capabilities. Skip it unless you are working on this project.

## Using it

**Start with the daemon and the client.** Two programs rather than one buys you the thing
that matters with a webcam: **only one process may stream from a camera at a time**, and the
daemon is what owns that. With it running, a script, a browser tab and you at a terminal can
all use the same camera; without it they take turns and lose.

```sh
webcam-handler-daemon &                      # owns the cameras, serves a Unix socket
webcam-handler-client list                   # every camera, with a stable id per camera
webcam-handler-client photo cam:my-camera -o shot.jpg
```

`webcam-handler-cli` is the same command surface with no daemon behind it — it opens the
device itself. Reach for it for a one-shot on a machine where nothing else wants the camera,
and for the daemon everywhere else.

Camera ids look like `cam:integrated-camera-integrated-c`. They are derived from what the
device says about itself, not from `/dev/video0`, because node numbers move between reboots
and between plug events — the id does not.

Add `--json` to any command to get the machine-readable form instead of a table; the shape is
committed under `schemas/` and validated in CI.

### Your user needs access to the camera

On a desktop system logind grants it to the seat owner automatically; otherwise:

```sh
sudo usermod -aG video "$USER"   # log out and back in
```

The tool says so itself, with the path it could not open, rather than failing obscurely.

### A calibration session, start to finish

Calibration answers "what settings make this camera see this scene well?", and it answers it
by **taking photos and scoring them** rather than by guessing. A session is a durable record
on disk: you can stop half way, come back, and see what happened.

The example tunes brightness on one camera, for one named task:

```sh
CAM=cam:my-camera

# 1. Open a session. The task name is how you find it again; the goal is for you.
webcam-handler-client calibrate start "$CAM" --task desk --goal "a legible whiteboard"

# 2. Draft the queue: which controls will be tuned, in what order.
webcam-handler-client calibrate plan "$CAM" --task desk brightness

# 3. Sweep. A photo per value, each one scored. `--points 9` samples nine values across
#    the control's range; `--step` or `--values` if you want to say exactly which.
webcam-handler-client calibrate sweep "$CAM" --task desk brightness --points 9

# 4. Look at what happened — every sample, its score, and how the sweep ended.
webcam-handler-client calibrate status "$CAM" --task desk

# 5. Choose. Either let a metric decide, or name the value yourself.
webcam-handler-client calibrate select "$CAM" --task desk brightness --metric sharpness
webcam-handler-client calibrate select "$CAM" --task desk brightness --value 140 --by me

# 6. Write the chosen values to the camera.
webcam-handler-client calibrate apply "$CAM" --task desk
```

Two things are worth knowing before you run it on a camera you care about.

**The session snapshots the camera before it touches anything**, and `calibrate restore` puts
it back:

```sh
webcam-handler-client calibrate restore "$CAM" --task desk
```

**A sweep takes real photos and moves real controls.** On a pan/tilt/zoom camera it will not
move the motors unless you pass `--allow-motion` — motors wear out, so moving them is
something you ask for rather than something that happens to you.

`calibrate list` shows every session on the machine, newest first, if you have forgotten what
you called one.

### The browser

The daemon can serve a web client — camera list, live preview, controls, and the calibration
view — but **it does not listen on TCP unless you ask**:

```sh
webcam-handler-daemon --http
```

It prints a URL with a single-use token in it. Open that URL. The listener is loopback-only by
default and the token is what stands between a stranger on your machine and your camera.

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
