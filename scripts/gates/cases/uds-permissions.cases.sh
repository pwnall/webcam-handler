# Both-direction cases for `uds-permissions.sh`.
#
# The subject is a *running daemon's* socket directory, not text in the tree, so the failing
# arms drive the predicate's documented seam — $WCH_GATE_WCHD, the daemon-shaped program it
# starts — at programs that get D11 wrong in one way each. `pass_case` drives the real
# `webcam-handler-daemon`, which is the arm rubric rule 6 requires [S:N10]: a predicate whose
# only input is injectable proves nothing about the repository.
#
# Each stub is a *plausible* wrong daemon rather than a nonsense one. They are the ways this
# has actually gone wrong — in other projects, and in this one: the umask decided the mode, the
# daemon announced before it bound, the daemon "helpfully" repaired a directory it should have
# refused, and the daemon checked the mode by *path* so a symlink answered for its target. The
# last of those is what `webcam-handler-daemon` itself did until the P4b review measured it
# (note N39).
#
# shellcheck shell=bash

# Where a daemon puts its socket, read from the crates that own the two names — the same
# derivation the predicate makes, for the same reason. A stub has to land its directory
# where the real daemon would, or the predicate would be looking somewhere else.
_socket_names() {
    local root
    root="$(gate_root)"
    sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
        "$root/crates/schema/src/paths.rs" | head -n1
    sed -n 's/^pub const DAEMON_SOCKET_FILE: &str = "\([^"]*\)".*/\1/p' \
        "$root/crates/schema/src/limits.rs" | head -n1
}

# Write a daemon-shaped program that prepares its socket directory as told, optionally binds
# nothing at all, announces the socket the way `webcam-handler-daemon` does, and exits. Exiting
# rather than serving is invisible to the predicate: it stops the daemon the moment it
# announces and then reads to end-of-file either way.
#
#   $1  where to write the program
#   $2  the mode to leave the directory at, whatever it was before
#   $3  `bind` to leave a file at the socket path, `announce-only` to leave none
#   $4  `serve` to announce, `refuse` to exit 1 without announcing
_stub_daemon() {
    local script="$1" mode="$2" bind="$3" behaviour="$4" app_dir socket_file
    { read -r app_dir; read -r socket_file; } < <(_socket_names)
    cat >"$script" <<STUB
#!/usr/bin/env bash
set -euo pipefail
if [[ "$behaviour" == refuse ]]; then
    printf 'webcam-handler-daemon cannot serve\n' >&2
    exit 1
fi
dir="\$XDG_RUNTIME_DIR/$app_dir"
mkdir -p "\$dir"
chmod $mode "\$dir"
if [[ "$bind" == bind ]]; then : >"\$dir/$socket_file"; fi
printf 'webcam-handler-daemon is serving socket=%s\n' "\$dir/$socket_file" >&2
STUB
    chmod +x "$script"
}

pass_case() {
    "$GATE"
}

# The umask decided the mode. This is what a daemon that creates its directory with
# `mkdir -p` and never looks at the result gets on a default-umask machine — and it is the
# defect the row exists for: every account on the box can then reach the camera.
fail_case_a_daemon_that_serves_from_a_world_traversable_directory() {
    local stub
    stub="$(mktemp "$(gate_scratch_root)/wch-stub-wchd.XXXXXXXX")"
    _stub_daemon "$stub" 0755 bind serve
    WCH_GATE_WCHD="$stub" "$GATE"
}

# The non-vacuity arm. A daemon that announces a socket it never bound would leave the
# predicate stat-ing an empty directory it made itself — the mode would be right and the
# check would mean nothing.
fail_case_a_daemon_that_announces_a_socket_it_never_bound() {
    local stub
    stub="$(mktemp "$(gate_scratch_root)/wch-stub-wchd.XXXXXXXX")"
    _stub_daemon "$stub" 0700 announce-only serve
    WCH_GATE_WCHD="$stub" "$GATE"
}

# The repair. This daemon's fresh directory is impeccable — so the first claim passes —
# but handed one that is already 0755 it `chmod`s it and serves anyway. That hides the
# fact that the directory was reachable, and for however long it was it may already have
# been reached (note N39). Nothing but this arm distinguishes it from the shipped daemon.
fail_case_a_daemon_that_repairs_a_widened_directory_instead_of_refusing() {
    local stub
    stub="$(mktemp "$(gate_scratch_root)/wch-stub-wchd.XXXXXXXX")"
    _stub_daemon "$stub" 0700 bind serve
    WCH_GATE_WCHD="$stub" "$GATE"
}

# The symlink. This daemon does everything the shipped one does *except* look at the
# object rather than the name: `stat` and `mkdir -p` both follow links, so it refuses a
# 0755 directory (claim 3 passes) and happily serves from a link to a 0700 one. Whoever
# owns the parent can then re-point that link, which on a host with a synthesised
# `$XDG_RUNTIME_DIR` under `/tmp` need not be us.
fail_case_a_daemon_that_serves_through_a_symlinked_socket_directory() {
    local stub app_dir socket_file
    { read -r app_dir; read -r socket_file; } < <(_socket_names)
    stub="$(mktemp "$(gate_scratch_root)/wch-stub-wchd.XXXXXXXX")"
    cat >"$stub" <<STUB
#!/usr/bin/env bash
set -euo pipefail
dir="\$XDG_RUNTIME_DIR/$app_dir"
mkdir -p "\$dir"
# By path, following whatever the name leads to — which is the whole defect.
mode="\$(stat -Lc %a "\$dir")"
if [[ "\$mode" != 700 ]]; then
    printf 'webcam-handler-daemon cannot serve: %s is mode %s\\n' "\$dir" "\$mode" >&2
    exit 1
fi
: >"\$dir/$socket_file"
printf 'webcam-handler-daemon is serving socket=%s\\n' "\$dir/$socket_file" >&2
STUB
    chmod +x "$stub"
    WCH_GATE_WCHD="$stub" "$GATE"
}

# A daemon that cannot start at all leaves this gate with nothing to examine, and a gate
# that examined nothing must not report a pass (AGENTS.md rule 3).
fail_case_a_daemon_that_never_serves_leaves_nothing_to_check() {
    local stub
    stub="$(mktemp "$(gate_scratch_root)/wch-stub-wchd.XXXXXXXX")"
    _stub_daemon "$stub" 0700 bind refuse
    WCH_GATE_WCHD="$stub" "$GATE"
}
