# Both-direction cases for `systemd-units.sh`.
#
# The subject is text in the tree, so every failing arm seeds one plausible mistake in a
# *copy* of it (`gate_scratch_tree`) and runs the real predicate over the result. They are the
# mistakes a person actually makes editing a unit file: reaching for `Type=forking` because
# that is what a daemon "is", leaving `DirectoryMode` at systemd's default, mistyping the
# socket path, and — the one this predicate mostly exists for — tuning one of the two stop
# bounds without the other.
#
# The last pair of arms is the point. `fail_case_a_stop_timeout_shorter_than_the_daemons_drain`
# edits the *unit* and `fail_case_a_drain_the_units_stop_timeout_cannot_fit` edits the
# *constant*, and neither edit breaks a build or a test: the daemon compiles, the suite is
# green, and the daemon is SIGKILLed halfway through the ordered teardown it was written to
# perform.
#
# Each arm names the sentence it is claiming (`gate_red_because`, note **N31**), and that pair is
# also where it earns its keep: both arms are red under the *same* sentence, and the only thing
# that separates "the unit was trimmed" from "the constant was raised" is which of the two
# numbers in it moved. So each of them claims its own seeded number. Three other seeds here are
# red under more than one branch — `Type=forking` trips the notify claim and the fork claim, an
# abstract address trips all three of the listen claims — and the arm names the one it is for.
#
# shellcheck shell=bash

# Where the unit files live, relative to a tree — one place, because every arm below has to
# reach the same two files the predicate walks.
_units() {
    printf '%s/packaging/systemd\n' "$1"
}

# Rewrite one directive in place, comments untouched.
#
#   $1  file  $2  key  $3  replacement value
_reset_directive() {
    local file="$1" key="$2" value="$3"
    sed "s|^\([[:space:]]*${key}[[:space:]]*=\).*|\1$value|" "$file" >"$file.seeded"
    mv "$file.seeded" "$file"
}

pass_case() {
    "$GATE"
}

# The second green arm, and it is about the *gate* rather than the units: `systemd.time(7)`
# spells a timeout several ways, and a predicate that understood only the one this repository
# happens to use would silently stop checking the pair the day somebody wrote it as `1min`.
# Both spellings below are longer than the daemon's 20 s drain, so both must stay green.
pass_case_a_stop_timeout_written_in_systemds_other_spellings() {
    local tree service
    tree="$(gate_scratch_tree)"
    service="$(_units "$tree")/wchd.service"
    _reset_directive "$service" TimeoutStopSec "1min"
    WCH_GATE_ROOT="$tree" "$GATE" || return 1

    _reset_directive "$service" TimeoutStopSec "45"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The shape a person reaches for when they think "daemon". systemd would then wait for a
# parent that never exits, give up, and afterwards signal whatever pid it guessed at — while
# `webcam-handler-daemon` sends a `READY=1` nobody is listening for.
fail_case_a_service_that_claims_to_fork() {
    local tree
    tree="$(gate_scratch_tree)"
    _reset_directive "$(_units "$tree")/wchd.service" Type "forking"
    gate_red_because 'is Type=forking and nothing in this workspace forks' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The same defect through its other keyword: a pid file is a claim that the process systemd
# started is not the process that serves.
fail_case_a_service_that_writes_a_pid_file() {
    local tree service
    tree="$(gate_scratch_tree)"
    service="$(_units "$tree")/wchd.service"
    printf 'PIDFile=%%t/webcam-handler/wchd.pid\n' >>"$service"
    gate_red_because 'has a PIDFile=' env WCH_GATE_ROOT="$tree" "$GATE"
}

# systemd's default `DirectoryMode` is 0755, so this is what *leaving the line out* produces —
# a socket directory every account on the machine can walk into, which is every account's
# route to a camera. The daemon refuses to serve from one; this arm is the half that keeps it
# from being handed one at all.
fail_case_a_socket_directory_anyone_can_walk_into() {
    local tree
    tree="$(gate_scratch_tree)"
    _reset_directive "$(_units "$tree")/wchd.socket" DirectoryMode "0755"
    gate_red_because 'has DirectoryMode=0755' env WCH_GATE_ROOT="$tree" "$GATE"
}

# The socket inode itself, left at what `bind(2)` gives it.
fail_case_a_world_writable_socket_inode() {
    local tree
    tree="$(gate_scratch_tree)"
    _reset_directive "$(_units "$tree")/wchd.socket" SocketMode "0666"
    gate_red_because 'has SocketMode=0666' env WCH_GATE_ROOT="$tree" "$GATE"
}

# A path that is *almost* right: the app directory is there and the file name is not. systemd
# binds it, the daemon serves it, and every client composes the other name and finds nothing.
fail_case_a_socket_unit_listening_where_no_client_looks() {
    local tree
    tree="$(gate_scratch_tree)"
    _reset_directive "$(_units "$tree")/wchd.socket" ListenStream "%t/webcam-handler/daemon.sock"
    gate_red_because 'listens on %t/webcam-handler/daemon.sock, which does not end in' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The abstract namespace, which is what somebody reaches for to avoid "leaving a file lying
# around". It has no directory, no mode and no owner, so D11's authentication does not exist —
# the daemon refuses it at run time (`socket-activation.sh` drives that), and a unit that asks
# for it is a unit that never starts the daemon.
fail_case_a_socket_unit_listening_in_the_abstract_namespace() {
    local tree
    tree="$(gate_scratch_tree)"
    _reset_directive "$(_units "$tree")/wchd.socket" ListenStream "@wchd"
    gate_red_because 'listens on the abstract namespace (@wchd)' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# Half of the pair this predicate exists for, edited from the unit's side: five seconds is a
# perfectly reasonable-looking number and it is a quarter of the daemon's own drain, so a stop
# that arrives during a calibration sweep is a SIGKILL rather than the ordered teardown.
fail_case_a_stop_timeout_shorter_than_the_daemons_drain() {
    local tree
    tree="$(gate_scratch_tree)"
    _reset_directive "$(_units "$tree")/wchd.service" TimeoutStopSec "5s"
    gate_red_because 'stops the daemon after 5000ms' env WCH_GATE_ROOT="$tree" "$GATE"
}

# The same defect from the *other* side, which is the arm that makes this a pair-argument
# rather than a check on a unit file: nobody touches the unit, and
# `limits::DAEMON_SHUTDOWN_DRAIN_MS` is raised past it. Everything still compiles, every test
# still passes, and the daemon is now killed mid-drain by a service manager whose bound
# nobody moved.
fail_case_a_drain_the_units_stop_timeout_cannot_fit() {
    local tree limits
    tree="$(gate_scratch_tree)"
    limits="$tree/crates/schema/src/limits.rs"
    sed 's/^pub const DAEMON_SHUTDOWN_DRAIN_MS: u64 = .*/pub const DAEMON_SHUTDOWN_DRAIN_MS: u64 = 90_000;/' \
        "$limits" >"$limits.seeded"
    mv "$limits.seeded" "$limits"
    gate_red_because "the daemon's own shutdown drain is 90000ms" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# A timeout the gate cannot read is a bound nothing is comparing, and it must not read as a
# pass — `systemd.time(7)` has no `45 fortnights`, but the failure that matters is a gate that
# shrugged at a value it did not understand.
fail_case_a_stop_timeout_in_units_nothing_can_read() {
    local tree
    tree="$(gate_scratch_tree)"
    _reset_directive "$(_units "$tree")/wchd.service" TimeoutStopSec "quickly"
    gate_red_because 'has TimeoutStopSec=quickly, which this gate cannot read' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# No units at all. A gate that examined nothing must not report a pass (AGENTS.md rule 3) —
# and "the packaging directory was lost in a merge" is exactly how a project ends up with a
# daemon nobody can install and a green CI run.
fail_case_a_tree_with_no_unit_files_to_check() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$(_units "$tree")"
    # Red on all three populations, which is what "no units at all" means; the service units are
    # the first of them and the one this arm is named for.
    gate_red_because 'examined zero service units' env WCH_GATE_ROOT="$tree" "$GATE"
}
