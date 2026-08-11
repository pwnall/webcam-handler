#!/usr/bin/env bash
#
# The daemon's socket directory is 0700 — docs/9's **UDS permissions** row (P4b), design
# D11, AGENTS.md "Hardware and privacy".
#
# D11 makes filesystem permissions the whole authentication model for `wchd`: there is no
# token, no peer-credential check and no allowlist on the Unix socket, because on Linux
# `connect(2)` needs search permission on every component of the path and the *directory*
# is therefore the boundary. The socket inode is not — it is created `0777 & ~umask` and
# asserting its mode would be asserting the wrong thing. So this predicate asks the one
# question that matters: **is the directory a shipped `wchd` actually serves from private,
# and does a `wchd` handed a directory that is not private refuse to serve?**
#
# ## Why a shell predicate, when docs/9's row reads like a Rust test
#
# The 0700 half *is* a Rust test — several, in `crates/daemon/src/uds.rs`, both directions,
# including the one that matters most (a pre-existing 0770 directory is refused and left
# alone). This predicate exists for the half a Rust test cannot report honestly: the
# other-uid check needs a second account and `sudo -n`, neither of which every runner has,
# and nextest has no runtime-skip concept — a `println!("SKIP")` inside a test that `just
# ci` runs without `--success-output final` is invisible, which is "skip == pass in a
# costume" (docs/8 Part C). `gate_skip` is counted and printed. Note N44 records the
# choice.
#
# ## What it drives, and the seam
#
# It runs a real `wchd` against a scratch pair of XDG directories, learns from the daemon's
# own stderr that it is serving — nothing here waits on a clock — and then inspects what it
# left behind. `timeout` is a watchdog, not synchronisation: a daemon that neither
# announces nor exits would otherwise hang CI forever.
#
# Inspecting the directory *after* the daemon has been stopped is deliberate and it is
# sound: this build never unlinks its socket (P4e-ii owns shutdown discipline), so the
# directory and the socket file are exactly as the daemon left them, and a mode is a
# property of a directory rather than of a process.
#
# $WCH_GATE_WCHD is the documented seam — the daemon-shaped program to drive. The selftest
# points it at programs that get D11 wrong in one way each; `pass_case` always drives the
# real `wchd`, which is rubric rule 6's requirement that one arm runs the real tool [S:N10].
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The mode, written here as *policy*. Deriving it from `daemon::uds::SOCKET_DIR_MODE`
# would produce a gate that follows the code it is meant to check: a build that widened
# the constant would widen the gate with it and stay green. D11 says 0700; this is the
# second opinion.
wanted_mode=700

# Where the daemon puts its socket, derived from the two crates that own the names —
# `engine::paths::APP_DIR` and `schema::limits::DAEMON_SOCKET_FILE`. Transcribing either
# would let a rename drift the gate away from the daemon silently (docs/9's derived-
# population rule), and this gate has to know the path in order to recognise the line the
# daemon logs when it starts serving.
app_dir="$(sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/engine/src/paths.rs" | head -n1)"
socket_file="$(sed -n 's/^pub const DAEMON_SOCKET_FILE: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/limits.rs" | head -n1)"

if [[ -z "$app_dir" || -z "$socket_file" ]]; then
    gate_fail "could not read the socket path out of the tree (APP_DIR=${app_dir:-?}, DAEMON_SOCKET_FILE=${socket_file:-?}); this gate cannot recognise a socket it cannot name"
    gate_finish
fi
gate_note "the socket this gate looks for is \$XDG_RUNTIME_DIR/$app_dir/$socket_file"

# The binary comes from the real checkout for `json-validates.sh`'s reason: building
# `wchd` inside each of the selftest's scratch copies would cost a full compile per case,
# and the subject here is the daemon's *behaviour*, which the seam below replaces wholesale
# when a case wants a different one.
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

# A watchdog on every daemon this gate starts. Generous, because it is not a timing
# assertion — it is the bound that turns "hangs forever" into "fails".
deadline=60

scratch="$(mktemp -d "${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}/wch-uds-permissions.XXXXXXXX")"
trap 'rm -rf "$scratch"' EXIT
# `mktemp -d` creates 0700, which would make the other-uid arm below pass for the wrong
# reason: a second account blocked by *our scratch directory* proves nothing about the
# daemon's. Everything this gate owns above the socket directory is deliberately
# traversable, so the socket directory is the only barrier left standing.
chmod 0755 "$scratch"

announced=0
transcript=""
served_status=0
daemons=0

# Start a daemon against a private pair of XDG directories and wait — on the daemon's own
# stderr, never on a clock — for it to announce the socket or exit. Stops it either way,
# so nothing this gate starts outlives it.
serve() {
    local runtime="$1" state="$2" socket fifo pid line
    socket="$runtime/$app_dir/$socket_file"
    daemons=$((daemons + 1))
    fifo="$scratch/stderr.$daemons"
    mkdir -p "$runtime" "$state"
    chmod 0755 "$runtime"
    mkfifo "$fifo"

    XDG_RUNTIME_DIR="$runtime" XDG_STATE_HOME="$state" RUST_LOG=info \
        timeout "$deadline" "$wchd" >/dev/null 2>"$fifo" &
    pid=$!

    announced=0
    transcript=""
    # Reading to end-of-file rather than breaking out on the announcement: the daemon is
    # signalled the moment it says it is serving, and the loop then drains the pipe until
    # the process is gone. That way nothing is ever writing into a pipe with no reader, and
    # a daemon that failed to start is reported with everything it printed.
    while IFS= read -r line; do
        transcript+="$line"$'\n'
        if [[ "$line" == *"$socket"* ]]; then
            announced=1
            kill "$pid" 2>/dev/null || true
        fi
    done <"$fifo"

    served_status=0
    wait "$pid" || served_status=$?
    rm -f "$fifo"
}

# ------------------------------------------------------------------ claim 1
#
# The directory a shipped `wchd` serves from is private, and owned by whoever ran it.

runtime="$scratch/run"
socket_dir="$runtime/$app_dir"
socket="$socket_dir/$socket_file"

serve "$runtime" "$scratch/state"

directories=0
if ((announced == 0)); then
    gate_fail "the daemon never announced a socket at $socket (exit $served_status); there is no socket directory to check the mode of"
    printf '%s' "$transcript" | sed 's/^/        /' >&2
else
    mode="$(stat -c %a "$socket_dir" 2>/dev/null || true)"
    owner="$(stat -c %U "$socket_dir" 2>/dev/null || true)"
    if [[ "$mode" != "$wanted_mode" ]]; then
        gate_fail "the daemon served from $socket_dir at mode ${mode:-<missing>}; D11 makes filesystem permissions the whole auth model and requires $wanted_mode"
    fi
    if [[ "$owner" != "$(id -un)" ]]; then
        gate_fail "$socket_dir is owned by ${owner:-<missing>}, not $(id -un); a per-user daemon's socket directory belongs to the user"
    fi
    if [[ -L "$socket_dir" ]]; then
        gate_fail "$socket_dir is a symlink; the mode above is its target's, and a link can be re-pointed between the check and the bind — D11's boundary is the directory itself"
    fi
    directories=1
    gate_note "the daemon served from $socket_dir (mode $mode, owner $owner)"

    # Non-vacuity: without this, a daemon that announced a path it never bound would have
    # its *empty* directory's mode checked and the gate would report a green it did not
    # earn. The socket file's own mode is deliberately not asserted — see the header.
    if [[ ! -e "$socket" ]]; then
        gate_fail "the daemon announced $socket and there is nothing there; the directory checked above is not one anything served from"
    fi
fi
gate_checked "$directories" "live socket directory(s) a shipped wchd served from, checked for mode $wanted_mode and owner"
gate_require_nonzero "$directories" "socket directories"

# ------------------------------------------------------------------ claim 2
#
# A second account cannot walk into it. `sudo -n` because a gate must never block on a
# password prompt — a check that hangs is worse than one that declines.

other=""
if command -v getent >/dev/null 2>&1; then
    # `nobody` by preference and by convention; failing that, any account that is neither
    # root nor the user running this. Derived from the passwd database rather than
    # transcribed, so a host without `nobody` still gets the check.
    if getent passwd nobody >/dev/null 2>&1 && [[ "$(id -un)" != "nobody" ]]; then
        other="nobody"
    else
        other="$(getent passwd | awk -F: -v me="$(id -un)" '$3 >= 1000 && $1 != me && $1 != "root" { print $1; exit }')"
    fi
fi

if [[ -z "$other" ]]; then
    gate_skip 1 "this host has no second account to try the socket directory as; the mode check above still ran, but nobody tried to walk past it"
elif ! sudo -n true 2>/dev/null; then
    gate_skip 1 "no non-interactive privilege is available (\`sudo -n true\` declines), so this run cannot become $other; the mode check above still ran, but nobody tried to walk past it"
elif ((directories == 0)); then
    gate_skip 1 "no socket directory was produced above, so there is nothing for $other to be refused by"
elif ! sudo -n -u "$other" test -x "$runtime" 2>/dev/null; then
    # The control arm, and it is the difference between a check and a coincidence: if
    # $other cannot even reach the *parent*, then being unable to reach the socket
    # directory says nothing about the socket directory's mode.
    gate_skip 1 "$other cannot traverse $runtime, so a refusal one level further down would prove nothing about the socket directory's own mode"
else
    if sudo -n -u "$other" test -x "$socket_dir" 2>/dev/null; then
        gate_fail "$other can walk into $socket_dir, which is every account on this machine's route to the camera; D11's auth model is that they cannot"
    fi
    gate_checked 1 "other-uid reachability check over the live socket directory, with the parent proven reachable first"
    gate_note "$other can traverse $runtime and cannot traverse $socket_dir"
fi

# ------------------------------------------------------------------ claim 3
#
# A directory that is already world-traversable is refused, not repaired. Create-with-0700
# proves nothing on its own: `mkdir` leaves a pre-existing directory's mode alone, and an
# `$XDG_RUNTIME_DIR/<app>` left 0755 by an older build, a tmpfiles rule or an operator is
# exactly the case D11 is about. A daemon that silently `chmod`ed it would hide that the
# directory had been reachable — and for however long it was, it may already have been
# reached (note N39).

widened_runtime="$scratch/widened"
widened_dir="$widened_runtime/$app_dir"
mkdir -p "$widened_dir"
chmod 0755 "$widened_dir"

serve "$widened_runtime" "$scratch/widened-state"

if ((announced == 1)); then
    gate_fail "the daemon served from $widened_dir, which is mode 0755; a socket directory anyone can walk into must be a refusal"
fi
if [[ -e "$widened_dir/$socket_file" ]]; then
    gate_fail "the daemon bound a socket in $widened_dir; nothing may be bound in a directory that is not $wanted_mode"
fi
left="$(stat -c %a "$widened_dir" 2>/dev/null || true)"
if [[ "$left" != "755" ]]; then
    gate_fail "the daemon changed $widened_dir from 0755 to ${left:-<missing>}; a wrong mode is refused rather than repaired, because a silent chmod hides that the directory was reachable"
fi
gate_checked 1 "startup refusal over a socket directory that is not private, with the mode asserted unchanged afterwards"

# ------------------------------------------------------------------ claim 4
#
# A socket directory that is a *symlink* is refused, however private its target. This is
# the same claim as claim 3 made about an object rather than about a name, and it is a
# separate arm because the two fail separately: `stat` and `create_dir_all` both follow
# links, so a daemon that checked the mode by path would pass claim 3 and serve from
# wherever the link pointed — after which whoever owns the parent can re-point it. On the
# many hosts that synthesise `$XDG_RUNTIME_DIR` under `/tmp` for a session, that parent is
# not necessarily ours (note N39).

linked_runtime="$scratch/linked"
linked_dir="$linked_runtime/$app_dir"
mkdir -p "$linked_runtime" "$scratch/link-target"
chmod 0700 "$scratch/link-target"
ln -s "$scratch/link-target" "$linked_dir"

serve "$linked_runtime" "$scratch/linked-state"

if ((announced == 1)); then
    gate_fail "the daemon served from $linked_dir, which is a symlink to $scratch/link-target; the mode it checked was the target's and the link can be re-pointed before the bind"
fi
if [[ -e "$scratch/link-target/$socket_file" ]]; then
    gate_fail "the daemon bound a socket through the symlink at $linked_dir; nothing may be bound in a directory reached by a name that can be re-pointed"
fi
gate_checked 1 "startup refusal over a socket directory that is a symlink to a private directory"

gate_finish
