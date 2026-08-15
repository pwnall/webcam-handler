# vendor/

## `v4l2-webcam-skill/` — superseded, and kept

`vendor/v4l2-webcam-skill/` is a submodule of
[pwnall/v4l2-webcam-skill](https://github.com/pwnall/v4l2-webcam-skill): an agent skill that
teaches an AI agent to drive a V4L2 webcam with `v4l2-ctl`, `ffmpeg`, `fuser` and `lsusb`. It
is read-only here, which is why this pointer sits beside it rather than in it.

**It is superseded by [`docs/agent-guide.md`](../docs/agent-guide.md).** That guide covers the
same operations, and it is generated from this tool's command surface, so it cannot describe a
verb or a flag the build does not have. Its last section maps each of the skill's manual
command sequences onto the call that replaces it.

The skill is **not wrong, and it was not a mistake**. It is the specification this project was
built from: design §1.1's operations map is the row-by-row correspondence between what the
skill teaches by hand and what `webcam-handler` does in one call, and every entry in it is a
requirement the skill wrote down first. `webcam-handler`'s claim is narrower than "the skill
was incorrect" — it is that a sequence of commands whose output has to be parsed is the wrong
shape for a caller with no hands, and that a tool talking to the kernel directly can answer
with a document instead.

Read it for the V4L2 background it explains, and for the `references/` notes on what a real
webcam does. Do not follow its command sequences to drive a camera on this machine: this tool
shells out to nothing, so `v4l2-ctl` and `ffmpeg` are not part of the runtime and their output
is not something anything here reads.

`scripts/gates/agent-guide-current.sh` checks that this file names the guide's real path, so a
guide that moves takes this pointer with it rather than leaving a reader at a dead end.
