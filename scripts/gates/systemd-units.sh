#!/usr/bin/env bash
#
# The shipped unit files say what the daemon needs them to say (docs/7 P4e-ii, design D9/D11,
# AGENTS.md "Hardware and privacy").
#
# A unit file is the one piece of this project that cannot ask a crate what a constant is. It
# is a text file installed under `~/.config/systemd/user/`, read by a service manager that has
# never heard of `schema::limits`, and every value in it is therefore a **transcription** —
# which is the defect class docs/9's derived-population rule is about. So this predicate
# re-derives each value from the tree and compares, rather than reading the unit files and
# nodding.
#
# ## The four claims, and why each one is separately worth a predicate
#
# 1. **`Type=notify`, and never `Type=forking`.** `Type=notify` is what makes "active
#    (running)" mean the socket is bound and answering rather than "a process was spawned";
#    the daemon sends `READY=1` for it (`daemon::systemd::Supervisor`). `Type=forking` is the
#    shape this project does not have — there is no `fork` in the workspace and no PID file —
#    and a unit that claimed it would leave systemd waiting for a parent to exit, then
#    tracking the wrong process, then signalling it on `systemctl stop`. A `PIDFile=` is the
#    same defect wearing a different keyword.
# 2. **`SocketMode=0600` and `DirectoryMode=0700`.** D11 makes filesystem permissions the whole
#    authentication model, and systemd's *default* `DirectoryMode` is 0755: a socket unit that
#    left it alone would create a directory every account on the machine can walk into, which
#    is every account's route to a camera pointed at somebody. `daemon::systemd::Activation`
#    refuses to serve from one — this is the half that stops it being reached at all.
# 3. **`ListenStream=` ends at the socket the daemon looks for.** The path is composed from
#    `schema::paths::APP_DIR` and `schema::limits::DAEMON_SOCKET_FILE` in the daemon and
#    written out by hand in the unit; both are read here, so a rename in either crate turns
#    this red instead of shipping a unit that binds a socket nothing connects to.
# 4. **`TimeoutStopSec` exceeds the whole of what the *process* takes to stop.** This is the
#    pair worth a predicate. The daemon's stop is an *order* (`daemon::shutdown`) bounded by its
#    own drain constant; systemd's `TimeoutStopSec` is when it stops asking and sends `SIGKILL`.
#    Whichever is smaller is the bound that fires, and if it is systemd's then the ordered
#    release never runs and every sentence this project writes about *how*
#    `webcam-handler-daemon` stops describes a path nothing takes. Tune either number alone —
#    raise the drain, trim the unit — and everything still compiles and every test still
#    passes.
#
#    **The comparison is against the sum and not against the drain alone, and that is a repair
#    rather than a refinement.** `daemon::shutdown`'s own header carries a four-wait table and
#    `limits::WEB_LISTENER_STOP_MS`'s doc says outright that its wait "happens *after* that
#    drain has already run, so it is added to it rather than shared with it" — three of the four
#    waits are sequential and the fourth needs no bound. This gate compared `TimeoutStopSec`
#    against `DAEMON_SHUTDOWN_DRAIN_MS` alone and never read the second constant at all, so a
#    drain raised to 21.5 s left the shipped unit green while systemd's SIGKILL landed exactly
#    on the ordered teardown the pair exists to protect (note **N304**). Both terms are derived
#    here now, and the summary line prints the sum with its terms so a reader can see which one
#    moved.
#
# ## What this predicate is not
#
# It is text over files, so it says nothing about whether systemd would *accept* the units.
# `systemd-analyze verify` is that check and it is not run here: it needs a systemd on the
# host, it resolves `ExecStart=` against a binary that is not installed in a checkout, and its
# verdict about a missing executable is not a verdict about this project. The runtime half —
# a real `webcam-handler-daemon` under a real service manager — is `socket-activation.sh`.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
units="$root/packaging/systemd"

# ------------------------------------------------------------------ derivation
#
# Every number and name below comes out of the crate that owns it. Transcribing one would
# produce a gate that agrees with the unit file for the same wrong reason it is wrong.

socket_file="$(sed -n 's/^pub const DAEMON_SOCKET_FILE: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/limits.rs" | head -n1)"
app_dir="$(sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/paths.rs" | head -n1)"
drain_ms="$(sed -n 's/^pub const DAEMON_SHUTDOWN_DRAIN_MS: u64 = \([0-9_]*\);.*/\1/p' \
    "$root/crates/schema/src/limits.rs" | head -n1)"
drain_ms="${drain_ms//_/}"

# The second term of the process's worst case, read the same way and from the same file. It is
# derived rather than folded into the first because `daemon::shutdown`'s table is what says how
# many of each there are, and a gate that summed a constant it had not read could not go red on
# either one moving.
web_stop_ms="$(sed -n 's/^pub const WEB_LISTENER_STOP_MS: u64 = \([0-9_]*\);.*/\1/p' \
    "$root/crates/schema/src/limits.rs" | head -n1)"
web_stop_ms="${web_stop_ms//_/}"

if [[ -z "$socket_file" || -z "$app_dir" || -z "$drain_ms" || -z "$web_stop_ms" ]]; then
    gate_fail "could not read the daemon's own names and bounds out of the tree (DAEMON_SOCKET_FILE=${socket_file:-?}, APP_DIR=${app_dir:-?}, DAEMON_SHUTDOWN_DRAIN_MS=${drain_ms:-?}, WEB_LISTENER_STOP_MS=${web_stop_ms:-?}); a gate that cannot derive them would be comparing the unit files against nothing"
    gate_finish
fi

# The sum `daemon::shutdown`'s four-wait table names: the ordered teardown, then the web
# listener's stop, then the blocking pool's join at the runtime's own shutdown — sequential, so
# they add. The watchdog join is the table's fourth row and carries no bound because it has
# nothing to wait for.
worst_ms=$((drain_ms + web_stop_ms + drain_ms))
gate_note "derived from the tree: socket \$XDG_RUNTIME_DIR/$app_dir/$socket_file, shutdown drain ${drain_ms}ms, web listener stop ${web_stop_ms}ms, process worst case ${worst_ms}ms"

# One directive's value, from the non-comment lines only.
#
# The comments in these unit files *argue about* the values they set — `wchd.service`'s header
# says in as many words that it is never `Type=forking` — so a grep that did not strip them
# would read the argument as the setting. Last-wins, which is systemd's own rule for a
# repeated directive.
directive() {
    local file="$1" key="$2"
    sed -n 's/^[[:space:]]*'"$key"'[[:space:]]*=[[:space:]]*\(.*\)$/\1/p' "$file" | tail -n1
}

# Whether a directive appears at all, comments excluded.
has_directive() {
    local file="$1" key="$2"
    grep -Eq "^[[:space:]]*${key}[[:space:]]*=" "$file"
}

# A systemd time span in milliseconds, for the one comparison that is arithmetic.
#
# `systemd.time(7)` lets `TimeoutStopSec=` be `45`, `45s`, `45sec`, `500ms` or `1min`, and a
# gate that understood only the spelling this repository happens to use today would go green
# on a unit whose bound had been rewritten in another one. Anything it cannot read is an empty
# answer and a failure at the call site, never a guess.
timespan_ms() {
    local span="${1// /}" number unit
    number="${span%%[a-zA-Z]*}"
    unit="${span#"$number"}"
    [[ "$number" =~ ^[0-9]+$ ]] || return 0
    case "$unit" in
    "" | s | sec | secs | second | seconds) printf '%s\n' "$((number * 1000))" ;;
    ms | msec | msecs) printf '%s\n' "$number" ;;
    m | min | mins | minute | minutes) printf '%s\n' "$((number * 60000))" ;;
    h | hr | hour | hours) printf '%s\n' "$((number * 3600000))" ;;
    *) return 0 ;;
    esac
}

# ------------------------------------------------------------------ population
#
# Derived by walking `packaging/systemd/`, not listed here: a third unit file added to the
# directory is a third unit file this gate reads.

mapfile -t -d '' services < <(gate_find "$units" -name '*.service')
mapfile -t -d '' sockets < <(gate_find "$units" -name '*.socket')

# ------------------------------------------------------------------ claim 1
#
# The service notifies, and never forks.

for service in "${services[@]}"; do
    name="${service#"$root/"}"
    type="$(directive "$service" Type)"
    if [[ "$type" != "notify" ]]; then
        gate_fail "$name is Type=${type:-<unset, which systemd reads as simple>}; \`webcam-handler-daemon\` sends READY=1 once its socket is bound (daemon::systemd::Supervisor), and only Type=notify makes \"active (running)\" mean a client's request will be answered"
    fi
    if [[ "$type" == "forking" ]]; then
        gate_fail "$name is Type=forking and nothing in this workspace forks; systemd would wait for a parent that never exits and then track the wrong process"
    fi
    if has_directive "$service" PIDFile; then
        gate_fail "$name has a PIDFile=; the process systemd starts is the process that serves, so a pid file is either a lie or a fork this project does not have"
    fi
    if [[ "$(directive "$service" NotifyAccess)" != "main" ]]; then
        gate_fail "$name does not set NotifyAccess=main; only the main process exists, and anything wider lets any process in the cgroup declare the camera daemon ready"
    fi
    if [[ "$(directive "$service" Restart)" != "on-failure" ]]; then
        gate_fail "$name does not Restart=on-failure; \`daemon::shutdown\` is careful to exit non-zero exactly when the daemon stopped serving without being asked to, and nothing acts on that unless this is set"
    fi
done
gate_checked "${#services[@]}" "service unit(s) checked for Type=notify, no fork, no PIDFile, NotifyAccess and Restart"
gate_require_nonzero "${#services[@]}" "service units"

# ------------------------------------------------------------------ claim 2 and 3
#
# The socket is private, its directory is private, and it is the socket the daemon looks for.

for socket in "${sockets[@]}"; do
    name="${socket#"$root/"}"
    mode="$(directive "$socket" SocketMode)"
    dir_mode="$(directive "$socket" DirectoryMode)"
    listen="$(directive "$socket" ListenStream)"

    if [[ "$mode" != "0600" ]]; then
        gate_fail "$name has SocketMode=${mode:-<unset, which is 0666>}; the socket inode is not D11's boundary but it is not somebody else's to write to either"
    fi
    if [[ "$dir_mode" != "0700" ]]; then
        gate_fail "$name has DirectoryMode=${dir_mode:-<unset, which systemd defaults to 0755>}; filesystem permissions on that directory are the whole of what authenticates this socket (D11), and a webcam-handler-daemon handed a socket inside a wider one refuses to start"
    fi
    if [[ -z "$listen" ]]; then
        gate_fail "$name has no ListenStream=; a socket unit that listens on nothing activates nothing"
    else
        if [[ "$listen" != */"$socket_file" ]]; then
            gate_fail "$name listens on $listen, which does not end in $socket_file — the name schema::limits::DAEMON_SOCKET_FILE gives it and the one every client composes"
        fi
        if [[ "$listen" != */"$app_dir"/* ]]; then
            gate_fail "$name listens on $listen, which is not inside a $app_dir directory — schema::paths::APP_DIR is the component the daemon and D11 both name"
        fi
        if [[ "$listen" == @* ]]; then
            gate_fail "$name listens on the abstract namespace ($listen), which has no directory, no mode and no owner; the daemon refuses one because D11's authentication would not exist"
        fi
    fi
    if [[ "$(directive "$socket" Accept)" == "yes" ]]; then
        gate_fail "$name has Accept=yes, which spawns a daemon per connection; \`webcam-handler-daemon\` serves many clients over one socket and bounds them with limits::DAEMON_MAX_CONNECTIONS, and one process per connection would be that many contenders for one state-directory lock (D9)"
    fi
done
gate_checked "${#sockets[@]}" "socket unit(s) checked for SocketMode 0600, DirectoryMode 0700, and a ListenStream ending in $app_dir/$socket_file"
gate_require_nonzero "${#sockets[@]}" "socket units"

# ------------------------------------------------------------------ claim 4
#
# The pair. See the header: whichever bound is smaller is the one that fires.

pairs=0
for service in "${services[@]}"; do
    name="${service#"$root/"}"
    span="$(directive "$service" TimeoutStopSec)"
    if [[ -z "$span" ]]; then
        gate_fail "$name sets no TimeoutStopSec; systemd's default is 90s, which is nowhere in this repository and cannot be reasoned about against limits::DAEMON_SHUTDOWN_DRAIN_MS"
        continue
    fi
    stop_ms="$(timespan_ms "$span")"
    if [[ -z "$stop_ms" ]]; then
        gate_fail "$name has TimeoutStopSec=$span, which this gate cannot read as a systemd time span; a bound nothing can compare is a bound nothing is checking"
        continue
    fi
    if ((stop_ms <= worst_ms)); then
        gate_fail "$name stops the daemon after ${stop_ms}ms and the process's own worst case is ${worst_ms}ms (2 x DAEMON_SHUTDOWN_DRAIN_MS + WEB_LISTENER_STOP_MS, the four-wait table in daemon::shutdown); the smaller bound is the one that fires, so this unit would SIGKILL a daemon in the middle of the ordered teardown daemon::shutdown exists to perform"
    else
        # Only on the green branch: this line says which bound fires, and printing it beside a
        # failure said the opposite of the failure above it.
        gate_note "$name: TimeoutStopSec=${stop_ms}ms > ${worst_ms}ms (${drain_ms} + ${web_stop_ms} + ${drain_ms}), so the daemon's own bound is what fires"
    fi
    pairs=$((pairs + 1))
done
gate_checked "$pairs" "(TimeoutStopSec, the four-wait worst case) pair(s), each term re-derived from the crate rather than transcribed"
gate_require_nonzero "$pairs" "stop-timeout pairs"

gate_finish
