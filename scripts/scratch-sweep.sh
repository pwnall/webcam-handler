#!/usr/bin/env bash
#
# Reclaim test scratch that no run is coming back for — `just scratch-sweep`.
#
# **A killed run cannot clean up after itself.** Every producer of scratch in this workspace
# deletes its own on the normal path (a `trap`, a `TempDir`'s `Drop`, `selftest.sh`'s per-arm
# `reclaim_scratch`), and not one of those runs after `kill -9`, an OOM kill, a full disk or a
# machine that went to sleep and never came back. That is not hypothetical: note **E15**
# measured **76 abandoned copies of this repository, 1 968 MB**, in a 16 GB tmpfs, all left by
# runs that were interrupted while a gate was being developed — and what it cost was not a red
# gate but a shell that intermittently refused to run anything, which is a very long way from
# the line that caused it.
#
# So cleanup has two halves and this is the second one: something that reclaims *afterwards*,
# which by definition cannot be the run that made the mess. `scripts/gates/selftest.sh` calls
# the same function at the start of every run with a day's grace; this script is the same
# function for a person, and it defaults to taking everything.
#
#   $1  the age in minutes below which an entry is presumed live. 0 — the default here — takes
#       every `wch*` entry in both roots, which is what you want when nothing is running and
#       is exactly what you do not want while a mutation floor is three hours into a run.
#
# What it does not cover is note N84's list as that note's own 2026-08-21 amendment leaves it,
# and the one worth remembering is a scratch root a caller pointed somewhere else with
# $WCH_GATE_SCRATCH. cargo-mutants' own `cargo-mutants-*` build trees (its name, not ours) came
# off the list when the owner moved the mutation floor's build root under this root (note
# N347): what this takes is the `wch…` directory above them, so they go with it — and they go
# back out of reach for whoever points $WCH_MUTANTS_BUILD_ROOT at another filesystem.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=gates/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/gates/lib.sh"

gate_scratch_sweep "${1:-0}"
