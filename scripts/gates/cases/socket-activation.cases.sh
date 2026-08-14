# Both-direction cases for `socket-activation.sh`.
#
# The subject is a *running daemon* handed a descriptor by a real service manager, not text in
# the tree, so the failing arms drive the predicate's documented seam — $WCH_GATE_WCHD, the
# daemon-shaped program it starts — at programs that get one of the four claims wrong.
# `pass_case` drives the real `webcam-handler-daemon`, which is the arm rubric rule 6 requires
# [S:N10].
#
# Each stub is a *plausible* wrong daemon rather than a nonsense one, and two of them are the
# real binary with one thing taken away — which is as close to "the defect that would actually
# ship" as a stub gets:
#
#   * a daemon that adopts whatever descriptor it is handed, because `from_raw_fd(3)` is two
#     lines and asking what the descriptor *is* is twenty;
#   * a daemon that never looks at `LISTEN_FDS` at all and binds its own socket over the top
#     of the one systemd bound — the real `webcam-handler-daemon` with the variables removed;
#   * a daemon that cannot start, leaving the gate nothing to examine;
#   * a daemon that renders its log to stderr under a journal that is already its stderr,
#     which is the same line in the journal twice.
#
# shellcheck shell=bash

# Where a daemon puts its socket, read from the crates that own the two names — the same
# derivation the predicate makes, for the same reason. A stub has to announce the path the
# real daemon would, or the predicate would be watching somewhere else.
_socket_names() {
    local root
    root="$(gate_root)"
    sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
        "$root/crates/schema/src/paths.rs" | head -n1
    sed -n 's/^pub const DAEMON_SOCKET_FILE: &str = "\([^"]*\)".*/\1/p' \
        "$root/crates/schema/src/limits.rs" | head -n1
}

# Where the real binary is, for the two stubs that are the real binary with something removed.
_real_wchd() {
    printf '%s/target/debug/webcam-handler-daemon\n' "$(git rev-parse --show-toplevel)"
}

_scratch_program() {
    mktemp "$(gate_scratch_root)/wch-stub-wchd.XXXXXXXX"
}

pass_case() {
    "$GATE"
}

# The second green arm, and it is about the **skip** rather than about the daemon: a host with
# no `systemd-socket-activate` must decline the three activation claims in a way that is named
# and counted, and still pass. A predicate that failed there would be unrunnable on every
# machine without systemd; one that passed *silently* would be "skip == pass in a costume",
# which is the thing note N44 says a shell predicate exists to avoid. The journald claim still
# runs here, so this arm is not a green nobody earned.
pass_case_a_host_that_cannot_pass_a_socket_in_declines_in_a_way_that_is_counted() {
    local heard
    heard="$(WCH_GATE_SOCKET_ACTIVATE="/nonexistent/systemd-socket-activate" "$GATE")" || return 1
    printf '%s\n' "$heard"
    grep -q "^  SKIP  " <<<"$heard"
}

# `from_raw_fd(3)` and serve. It is the shortest thing that works, it passes the adoption claim
# — the descriptor really is the one systemd bound — and it is wrong about both refusals: an
# abstract address has no directory to authenticate with, and two descriptors have no rule
# saying which is the JSON-RPC socket. It is also wrong about the journal, because it renders
# its own line to stderr.
fail_case_a_daemon_that_adopts_whatever_descriptor_it_is_handed() {
    local stub app_dir socket_file
    { read -r app_dir; read -r socket_file; } < <(_socket_names)
    stub="$(_scratch_program)"
    cat >"$stub" <<STUB
#!/usr/bin/env bash
set -euo pipefail
dir="\$XDG_RUNTIME_DIR/$app_dir"
mkdir -p "\$dir"
printf 'webcam-handler-daemon is serving socket=%s\n' "\$dir/$socket_file" >&2
# Stay up until it is signalled, the way a daemon does; the predicate stops it the moment it
# reads the line above.
while :; do sleep 1; done
STUB
    chmod +x "$stub"
    WCH_GATE_WCHD="$stub" "$GATE"
}

# The real `webcam-handler-daemon` with `LISTEN_FDS` taken away, which is exactly a daemon that
# never implemented socket activation: it binds its own socket at D11's path, over the top of
# the one systemd bound and is still holding. Every client that connected to the first is
# talking to an unlinked inode, and nothing about the daemon looks wrong — which is why the
# claim is asserted on the inode rather than on the path.
fail_case_a_daemon_that_binds_its_own_socket_over_the_one_it_was_handed() {
    local stub real
    real="$(_real_wchd)"
    stub="$(_scratch_program)"
    cat >"$stub" <<STUB
#!/usr/bin/env bash
set -euo pipefail
exec env -u LISTEN_FDS -u LISTEN_PID "$real" "\$@"
STUB
    chmod +x "$stub"
    WCH_GATE_WCHD="$stub" "$GATE"
}

# A daemon that cannot start at all leaves this gate with nothing to examine, and a gate that
# examined nothing must not report a pass (AGENTS.md rule 3).
fail_case_a_daemon_that_never_serves_leaves_nothing_to_check() {
    local stub
    stub="$(_scratch_program)"
    cat >"$stub" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf 'webcam-handler-daemon cannot serve\n' >&2
exit 1
STUB
    chmod +x "$stub"
    WCH_GATE_WCHD="$stub" "$GATE"
}

# The journald half, and the only stub here that is *right* about all three activation claims:
# with a descriptor passed in it is the real binary, and without one — which is how the
# transient unit starts it — it renders a log line to stderr instead of writing a structured
# entry. Under a unit, stderr already is the journal, so that line arrives as
# `_TRANSPORT=stdout` and every line the daemon logs is in the journal twice (design §2.6).
fail_case_a_daemon_that_renders_its_log_to_a_stderr_that_is_already_the_journal() {
    local stub real app_dir socket_file
    { read -r app_dir; read -r socket_file; } < <(_socket_names)
    real="$(_real_wchd)"
    stub="$(_scratch_program)"
    cat >"$stub" <<STUB
#!/usr/bin/env bash
set -euo pipefail
if [[ -n "\${LISTEN_FDS:-}" ]]; then
    exec "$real" "\$@"
fi
dir="\$XDG_RUNTIME_DIR/$app_dir"
mkdir -p "\$dir"
printf 'webcam-handler-daemon is serving socket=%s\n' "\$dir/$socket_file" >&2
while :; do sleep 1; done
STUB
    chmod +x "$stub"
    WCH_GATE_WCHD="$stub" "$GATE"
}
