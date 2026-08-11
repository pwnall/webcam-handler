#!/usr/bin/env bash
#
# A real `wchd` under a real service manager (docs/7 P4e-ii, design D11, §2.6).
#
# `daemon::systemd`'s unit tests take every decision in that module apart over values, and
# `crates/daemon/tests/systemd.rs` drives a real daemon against a notify socket this project
# binds itself. Neither can produce the two things only a service manager produces: a
# **descriptor passed in through `LISTEN_FDS`**, and a **stderr that is the journal**. This
# predicate is where those two are driven, with the real tools, against the real binary.
#
# ## Why a shell predicate and not a `#[test]`
#
# Note **N44**'s reason, restated: neither `systemd-socket-activate` nor a running user
# journal is on every machine this suite runs on, and nextest has no runtime-skip concept — a
# `println!("SKIP")` inside a test that `just ci` runs without `--success-output final` is
# invisible, which is "skip == pass in a costume" (docs/8 Part C). `gate_skip` is named and
# counted, and a reader of CI output can see exactly which claims this host proved and which
# it declined.
#
# ## The four claims
#
# 1. **The daemon serves the socket systemd bound, and never binds its own.** Asserted on the
#    socket's **inode**, read once when the activator says it is listening and again once the
#    daemon says it is serving: a daemon that unlinked the inherited socket and bound its own
#    would leave a different inode at the same path, and every client that had connected to
#    the first would be talking to nothing. The path the daemon announces is checked as well,
#    because binding the right inode and announcing another would be a daemon nobody can find.
# 2. **An abstract-namespace socket is refused.** `-l @name` binds one with no filesystem
#    presence at all: no directory, no mode, no owner. D11 makes filesystem permissions the
#    whole of this daemon's authentication, so a `wchd` that served one would be serving a
#    camera to every process in the network namespace that can spell the name.
# 3. **More than one inherited descriptor is refused.** This daemon serves one socket. Picking
#    the first of several would be a guess an operator cannot see.
# 4. **When stderr really is the journal, the log goes there as structured entries.** Design
#    §2.6 asks for a journald layer "under systemd", and `daemon::logging` installs it
#    *instead of* the stderr formatter — both would put every line in the journal twice. The
#    check is `_TRANSPORT`: an entry the journald layer wrote is `journal`, and the same line
#    rendered to a stderr that systemd captured is `stdout`. This is the half
#    `crates/daemon/tests/systemd.rs` cannot drive, because it needs a real
#    `/run/systemd/journal/socket` and a user manager to start a transient unit under.
#
# ## What it drives, and the seam
#
# $WCH_GATE_WCHD is the documented seam — the daemon-shaped program to start. The selftest
# points it at programs that get one of these wrong each; `pass_case` always drives the real
# `wchd`, which is rubric rule 6's requirement that one arm runs the real tool [S:N10].
# $WCH_GATE_SOCKET_ACTIVATE is the other seam, and it exists for the skip: a case can point it
# at a program that is not there and watch this predicate decline in a way that is counted.
#
# `timeout` is a watchdog and not synchronisation: every wait here is a read on the daemon's
# own stderr, which blocks until the daemon writes and ends when it exits.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# Where the daemon's socket goes, derived from the two crates that own the names —
# `schema::paths::APP_DIR` and `schema::limits::DAEMON_SOCKET_FILE`. This gate has to compose
# the path it asks systemd to bind, and transcribing either name would let a rename drift the
# gate away from the daemon silently (docs/9's derived-population rule).
app_dir="$(sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/paths.rs" | head -n1)"
socket_file="$(sed -n 's/^pub const DAEMON_SOCKET_FILE: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/limits.rs" | head -n1)"

if [[ -z "$app_dir" || -z "$socket_file" ]]; then
    gate_fail "could not read the socket path out of the tree (APP_DIR=${app_dir:-?}, DAEMON_SOCKET_FILE=${socket_file:-?}); this gate cannot ask systemd to bind a socket it cannot name"
    gate_finish
fi
gate_note "the socket this gate has systemd bind is \$XDG_RUNTIME_DIR/$app_dir/$socket_file"

# The binary comes from the real checkout for `uds-permissions.sh`'s reason: building `wchd`
# inside each of the selftest's scratch copies would cost a full compile per case, and the
# subject here is the daemon's *behaviour*, which the seam below replaces wholesale when a
# case wants a different one.
checkout="$(git rev-parse --show-toplevel)"
wchd="${WCH_GATE_WCHD:-$checkout/target/debug/wchd}"
if [[ -n "${WCH_GATE_WCHD:-}" ]]; then
    gate_note "driving the daemon-shaped program at $wchd (WCH_GATE_WCHD)"
elif [[ ! -x "$wchd" ]]; then
    (cd "$checkout" && cargo build --locked --offline -p webcam-handler-daemon --bin wchd >/dev/null 2>&1) ||
        {
            gate_fail "could not build wchd; this gate has nothing to drive"
            gate_finish
        }
fi
if [[ ! -x "$wchd" ]]; then
    gate_fail "$wchd is not an executable program"
    gate_finish
fi

activate="${WCH_GATE_SOCKET_ACTIVATE:-systemd-socket-activate}"
# A watchdog on every daemon and every journal read this gate starts. Generous, because
# neither is a timing assertion — they are the bound that turns "hangs forever" into "fails".
# The journal read gets a smaller one only because the thing it waits for is a line the daemon
# has already written by the time this gate looks.
deadline=60
journal_deadline=20

scratch="$(mktemp -d "${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}/wch-socket-activation.XXXXXXXX")"
trap 'rm -rf "$scratch"' EXIT

announced=0
transcript=""
bound_inode=""
serving_inode=""
runs=0

# Start `wchd` under the activator with the given `--listen` addresses, and wait — on the
# daemon's own stderr, never on a clock — for it to announce the socket or exit.
#
#   $1  the private runtime directory to give it
#   $2  the socket path whose inode is watched (empty when there is nothing on the filesystem)
#   $@  the addresses to hand `--listen`
activated() {
    local runtime="$1" watched="$2" fifo pid line
    shift 2
    runs=$((runs + 1))
    fifo="$scratch/stderr.$runs"
    local state="$runtime.state"
    mkdir -p "$runtime/$app_dir" "$state"
    # The socket directory is D11's 0700, made by this gate because on this path it is
    # systemd's `DirectoryMode=` that would make it — `systemd-units.sh` is what asserts the
    # unit says so, and this gate is about what the daemon does with what it is handed.
    chmod 0700 "$runtime/$app_dir"
    mkfifo "$fifo"

    local addresses=()
    local address
    for address in "$@"; do
        addresses+=(--listen "$address")
    done

    # `--now` starts the child immediately instead of waiting for a connection, which is what
    # lets this gate assert what the daemon does with the descriptor without needing a client
    # — `wchc` is P4f's and there is no other JSON-RPC-over-UDS client in this tree.
    #
    # `-E` for each variable because the activator does **not** pass its own environment
    # through: measured on this host, a `wchd` started without them refused with
    # "$XDG_STATE_HOME: unset", which is the daemon's own startup refusal and not this claim.
    timeout "$deadline" "$activate" --now "${addresses[@]}" \
        -E "XDG_RUNTIME_DIR=$runtime" -E "XDG_STATE_HOME=$state" -E "RUST_LOG=info" \
        -- "$wchd" >/dev/null 2>"$fifo" &
    pid=$!

    announced=0
    transcript=""
    bound_inode=""
    serving_inode=""
    # Reading to end-of-file rather than breaking out on the announcement: the daemon is
    # signalled the moment it says it is serving, and the loop then drains the pipe until the
    # process is gone. Reaching end-of-file is itself a claim — a daemon that had forked into
    # the background would leave a child holding this pipe open — and it is what makes the
    # `wait` below return.
    while IFS= read -r line; do
        transcript+="$line"$'\n'
        # The activator says this *before* it execs the daemon, so an inode read here is the
        # inode systemd bound. That ordering is the whole of claim 1's soundness.
        if [[ "$line" == *"Listening on"* && -n "$watched" ]]; then
            bound_inode="$(stat -c %i "$watched" 2>/dev/null || true)"
        fi
        # "It announced" is either half: the path this run is watching, or the sentence a
        # daemon says when it is serving. The second half is what stops a *wrong* daemon from
        # holding this gate until the watchdog fires — on the two claims below there is no
        # path to watch, because the correct answer is a refusal and an exit.
        if { [[ -n "$watched" && "$line" == *"$watched"* ]]; } || [[ "$line" == *"is serving"* ]]; then
            announced=1
            [[ -n "$watched" ]] && serving_inode="$(stat -c %i "$watched" 2>/dev/null || true)"
            kill "$pid" 2>/dev/null || true
        fi
    done <"$fifo"

    wait "$pid" 2>/dev/null || true
    rm -f "$fifo"
}

if ! command -v "$activate" >/dev/null 2>&1 && [[ ! -x "$activate" ]]; then
    # Named and counted, never silence (AGENTS.md rule 3). The three claims below are about a
    # descriptor a service manager passes in, and this host has nothing that passes one; the
    # refusals themselves are also unit-tested over values in `daemon::systemd`, which is what
    # a host without systemd still gets.
    gate_skip 3 "$activate is not on this host, so nothing can pass this daemon a socket; the adoption, the abstract-address refusal and the too-many-descriptors refusal all rest on daemon::systemd's unit tests here"
else
    # -------------------------------------------------------------- claim 1
    adopting="$scratch/adopting"
    runtime_socket="$adopting/$app_dir/$socket_file"
    activated "$adopting" "$runtime_socket" "$runtime_socket"

    served=0
    if ((announced == 0)); then
        gate_fail "the daemon never announced the socket systemd bound at $runtime_socket; there is nothing to compare"
        printf '%s' "$transcript" | sed 's/^/        /' >&2
    else
        if [[ -z "$bound_inode" ]]; then
            gate_fail "the activator never reported binding $runtime_socket, so this gate has no inode to compare the served one against"
        elif [[ "$bound_inode" != "$serving_inode" ]]; then
            gate_fail "the socket at $runtime_socket was inode $bound_inode when systemd bound it and inode ${serving_inode:-<gone>} once the daemon was serving; the daemon replaced the socket it was handed, so every client holding a connection to the first is talking to nothing"
        else
            served=1
            gate_note "the daemon served inode $serving_inode at $runtime_socket — the one systemd bound, not one of its own"
        fi
    fi
    gate_checked "$served" "activated daemon(s) serving the very socket systemd bound, compared by inode"
    gate_require_nonzero "$served" "activated daemons"

    # -------------------------------------------------------------- claim 2
    #
    # An abstract address has nothing on the filesystem, so there is no inode to watch and no
    # path for the daemon to announce: the assertion is that it announces *nothing* and exits.
    activated "$scratch/abstract" "" "@wch-gate-$$"
    if ((announced == 1)); then
        gate_fail "the daemon served a socket in the abstract namespace; it has no directory, no mode and no owner, so D11's authentication does not exist for it and every process that can spell the name reaches the camera"
        printf '%s' "$transcript" | sed 's/^/        /' >&2
    fi
    gate_checked 1 "startup refusal over an abstract-namespace socket passed in through LISTEN_FDS"

    # -------------------------------------------------------------- claim 3
    #
    # Both addresses under the same private directory, so what is being refused is the *count*
    # and not a mode: a daemon that took the first of two would pass every other claim here.
    several="$scratch/several"
    activated "$several" "" \
        "$several/$app_dir/$socket_file" "$several/$app_dir/second.sock"
    if ((announced == 1)); then
        gate_fail "the daemon served with two descriptors passed in; it serves one socket (D11), and picking one of two would be a guess an operator cannot see"
        printf '%s' "$transcript" | sed 's/^/        /' >&2
    fi
    gate_checked 1 "startup refusal over two descriptors passed in through LISTEN_FDS"
fi

# ------------------------------------------------------------------ claim 4
#
# stderr *is* the journal. Only a service manager can arrange that — `$JOURNAL_STREAM` is set
# by the manager that started the process, not by anything a script can do to itself — so this
# is a transient user unit, read back through the journal it wrote to.

journal_socket=/run/systemd/journal/socket
if ! command -v systemd-run >/dev/null 2>&1 || ! command -v journalctl >/dev/null 2>&1; then
    gate_skip 1 "systemd-run or journalctl is not on this host, so nothing here can start a unit whose stderr is the journal; daemon::systemd's unit tests cover the comparison itself and crates/daemon/tests/systemd.rs covers the non-matching direction"
elif [[ ! -S "$journal_socket" ]]; then
    gate_skip 1 "$journal_socket is not there, so there is no journal for a daemon's stderr to be; the journald layer's own construction is what would fail, and daemon::logging falls back to stderr with a warning"
elif ! systemctl --user show-environment >/dev/null 2>&1; then
    gate_skip 1 "there is no systemd user manager for this session, so a transient unit cannot be started; the fallback direction is covered by crates/daemon/tests/systemd.rs"
else
    unit="wch-gate-journal-$$"
    runtime="$scratch/journal-run"
    state="$scratch/journal-state"
    mkdir -p "$runtime/$app_dir" "$state"
    chmod 0700 "$runtime/$app_dir"

    started=0
    if systemd-run --user --unit="$unit" --collect \
        -p "Environment=XDG_RUNTIME_DIR=$runtime" \
        -p "Environment=XDG_STATE_HOME=$state" \
        -p "Environment=RUST_LOG=info" \
        "$wchd" >/dev/null 2>&1; then
        started=1
    fi

    if ((started == 0)); then
        gate_fail "could not start a transient user unit for $wchd; this gate has nothing whose stderr is the journal"
    else
        # `journalctl -f` blocks until the entry arrives, which is a read on the journal
        # rather than a wait on a clock; `timeout` is the watchdog that turns "never" into a
        # failure. The daemon's readiness line is the needle, because it is the one line every
        # other suite in this project already waits for.
        entry="$(timeout "$journal_deadline" journalctl --user --unit="$unit" --output=json \
            --output-fields=MESSAGE,_TRANSPORT --follow --lines=50 2>/dev/null |
            grep -m1 '"MESSAGE"[^}]*wchd is serving' || true)"
        systemctl --user stop "$unit" >/dev/null 2>&1 || true
        systemctl --user reset-failed "$unit" >/dev/null 2>&1 || true

        if [[ -z "$entry" ]]; then
            gate_fail "the daemon started as a transient unit never said it was serving in the journal; there is nothing to check the transport of"
        elif [[ "$entry" != *'"_TRANSPORT":"journal"'* ]]; then
            gate_fail "the daemon's log line reached the journal with $(printf '%s' "$entry" | grep -o '"_TRANSPORT":"[^"]*"') rather than _TRANSPORT=journal; its stderr already is the journal, so a line that arrived that way was rendered by the fmt layer and every line is in the journal twice (design §2.6, daemon::logging)"
        else
            gate_note "the daemon's readiness line reached the journal as a structured entry (_TRANSPORT=journal), so the journald layer replaced the stderr formatter rather than joining it"
        fi
        gate_checked 1 "log line(s) from a daemon whose stderr is a real journal, checked for the transport that says which layer wrote it"
    fi
fi

gate_finish
