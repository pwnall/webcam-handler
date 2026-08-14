#!/usr/bin/env bash
#
# `webcam-handler-cli` and `webcam-handler-client` are one surface — docs/9's **CLI parity**
# row (P4f), design T4, T5, D9.
#
# T4 says a verb exists once: `webcam-handler-cli-core` owns the clap tree, the argument types
# and both renderings, and the two binaries differ only in an `Executor` — an engine in this
# process for `webcam-handler-cli`, a socket for `webcam-handler-client`. That is a claim about
# *behaviour*, and the smallest place it is observable from outside is the `--json` document,
# which is "the schema document and nothing else — no envelope, no timestamp, no tool version"
# (`cli_core::render`). So this predicate runs the two shipped binaries against one daemon
# replaying one committed profile and compares the bytes.
#
# ## The population is the surface's own, and every leaf lands in a named bucket
#
# The leaf verbs are scraped from `webcam-handler-cli --help` at **both** levels —
# `json-validates.sh`'s idiom and the P3 review's finding, because two of the ten top-level
# names are subtrees and a single-level scrape once let one row answer for a seven-verb tree.
# Each leaf must match a row below **exactly**, and a leaf in no row is a failure: that is what
# makes the table complete rather than transcribed. Rows that name no leaf fail too, so a verb
# deleted from the surface cannot leave a row here quietly answering for it.
#
# Four buckets, and three of them carry a check that their own reason implies — an exemption
# whose reason nothing tests is the "silently exempted" docs/9 forbids, wearing a label:
#
# - **compared** — the read verbs. Both roots answer, both exit 0, and the bytes are equal.
# - **exempt, session** — writes D9's session tree. Comparing them would mean two sessions in
#   two states, and `webcam-handler-cli` cannot run one beside a live `webcam-handler-daemon`
#   at all: D9 gives the daemon the store lock for its lifetime, and a `webcam-handler-cli`
#   meeting it is refused with `LockProtocol::HeldForLifetime`'s advice. *Checked both ways*:
#   `webcam-handler-client` answers it, and `webcam-handler-cli` is refused, in that refusal's
#   own words.
# - **exempt, device** — drives and mutates the camera. `webcam-handler-cli` and
#   `webcam-handler-daemon` each replay the profile into a device state **of their own**, so a
#   comparison would write to two devices and read back two answers about two of them.
#   *Checked*: `webcam-handler-client` answers it.
# - **exempt, stamped** — `snapshot` reads the camera and mutates nothing, but
#   `Snapshot::taken_at` stamps the instant of the read, so two invocations differ by
#   construction. It is deliberately **not** filed as mutating, because it is not.
#   *Checked*: with that one field removed the two answers are equal, and with it they are
#   not — the exemption proved to be exactly one field wide rather than asserted.
#
# The honest limit, recorded rather than left to be discovered: this predicate catches a row
# that is **missing**, a row whose bucket is not one of the four, and — through the three
# checks above — a read verb *relabelled* into `session` or `stamped`. A read verb relabelled
# into `device` would pass, because the only checkable consequence of "it mutates the camera"
# is the double drive that exemption exists to avoid performing. Review carries that one.
#
# ## Why the two argv differ in their global flags, and why the answers are still comparable
#
# `webcam-handler-cli` is handed `--backend fake --profile <profile>` and
# `webcam-handler-client` must **not** be: those two flags name a composition root's decision,
# `webcam-handler-client` is not one, and P4f made it refuse them rather than ignore them
# (`webcam-handler-client`'s root header argues the three available answers). That is a
# difference in how each process is *pointed at a camera*, not in what the verb is — the daemon
# was started on the same committed profile, so both ends resolve the same replayed device and
# the document under comparison is the verb's answer about it. A flag that selects a backend
# sits upstream of the surface T4 shares; if it did not, `webcam-handler-daemon --backend` and
# `webcam-handler-cli --backend` would be two spellings of one decision and the parity claim
# would be about the wrong thing. So this is not an exemption and is not recorded as one.
#
# ## `terminate_holder` is out of this population by construction
#
# It reached the T5 wire at P4c with no T4 spelling at all (note **N48**: "nothing here
# schedules it"), so `webcam-handler-cli --help` does not offer it and it is not a leaf. It is
# absent because the surface does not have it, not because this table excused it — which is the
# distinction docs/9 asks for when it says nothing may be silently absent.
#
# ## The load-bearing property, which this gate also happens to exercise
#
# One `webcam-handler-daemon` holds the state directory for its whole lifetime, and this gate
# runs `webcam-handler-cli` beside it against **the same** `$XDG_STATE_HOME`. That works only
# because `webcam-handler-cli`'s *read* verbs take no lock under either D9 protocol —
# `crates/daemon/src/state.rs`'s header argues exactly this — so `webcam-handler-cli calibrate
# status` answers a session a daemon owns while the daemon owns it, and `webcam-handler-cli
# calibrate plan` does not. Both halves are asserted here, one per bucket, so the property the
# fixture rests on is a thing this gate checks rather than a thing it assumes.
#
# ## The table is ordered, and the order is part of the fixture
#
# Row order is execution order, for `json-validates.sh`'s reason and one more. Its reason: a
# calibration verb's answer only exists once the verbs before it have run, so the session rows
# run `start` → `plan` → `sweep` → `select` → `apply` → `restore`, and the two compared
# session-reading rows come after them. The extra one: the compared rows whose subject is the
# *camera* run **before** anything writes to it, because `webcam-handler-cli` replays the
# profile fresh in its own process while `webcam-handler-client` reads a device this gate has
# been driving — so a `get` compared after a `set` would be comparing two different device
# states and calling it a defect.
#
# ## What it drives, and the seams
#
# `$WCH_GATE_WCH`, `$WCH_GATE_WCHC` and `$WCH_GATE_WCHD` are the documented seams — the three
# programs to drive. The selftest points the first two at a build carrying a seeded fork in
# the shared render path (real binaries, built from a scratch tree) and the third at daemons
# that get the fixture wrong. `pass_case` always drives the shipped three, which is rubric
# rule 6's requirement that one arm runs the real tool [S:N10].
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# ------------------------------------------------------------------ the table
#
# `<camera>`, `<control>`, `<value>`, `<snapshot>`, `<photo>` and `<session>` are substituted
# from the corpus and from documents earlier rows produce; nothing below is transcribed. See
# the header for the buckets and for why the order matters.
verbs=(
    "list|compared|list"
    "info|compared|info <camera>"
    "controls|compared|controls <camera>"
    "get|compared|get <camera> <control>"
    "snapshot|stamped|snapshot <camera>"
    "set|device|set <camera> <control>=<value>"
    "restore|device|restore <camera> <snapshot>"
    "photo|device|photo <camera> -o <photo>"
    "profile-capture|device|profile capture <camera>"
    "calibrate-start|session|calibrate start <camera> --task gate --goal gate-run"
    "calibrate-plan|session|calibrate plan <camera> --task gate <control>"
    "calibrate-sweep|session|calibrate sweep <camera> --task gate <control> --values <value> --skip-frames 0"
    "calibrate-select|session|calibrate select <camera> --task gate <control> --metric sharpness"
    "calibrate-apply|session|calibrate apply <camera> --task gate"
    "calibrate-restore|session|calibrate restore <camera> --task gate"
    "calibrate-status|compared|calibrate status <camera> --session <session>"
    "calibrate-list|compared|calibrate list <camera>"
)

# The closed vocabulary. A bucket outside it is a failure rather than a fall-through, because
# a fall-through is precisely how a verb gets silently exempted: one typo and an `else` that
# means "skip" excuses a row nobody notices.
buckets=(compared session device stamped)

# ------------------------------------------------------------------ the corpus
#
# Read out of the tree exactly as `json-validates.sh` reads it, and for its reason: a
# re-capture that renames a control or a card must break this loudly rather than quietly.

# The slug of a control this gate may write: integer, writable, holding a value inside its
# own declared range. `0x0004` is READ_ONLY and `0x0001` is DISABLED, both checked against
# the raw flag word because that is what the profile records.
writable_control() {
    jq -r '
        [ .invariant.controls[]
          | select(.type.kind == "integer")
          | select((.flags.raw // 0) % 2 == 0)
          | select((((.flags.raw // 0) / 4) | floor) % 2 == 0)
          | select(.range.max > .range.min)
        ] | first | if . == null then "" else "\(.slug)\t\(.default)\t\(.range.min)\t\(.range.max)" end
    ' "$1" 2>/dev/null
}

# Sorted, and the first with a writable integer control: the same subject on every run and in
# every scratch copy, which is what keeps this gate's green meaning one thing.
profile=""
for candidate in $(gate_find "$root/corpus/profiles" -name '*.json' | tr '\0' '\n' | sort); do
    if [[ -n "$(writable_control "$candidate")" ]]; then
        profile="$candidate"
        break
    fi
done
if [[ -z "$profile" ]]; then
    gate_fail "no committed profile exposes a writable integer control, so the write verbs this table exempts could not be driven through webcam-handler-client at all"
    gate_finish
fi

camera_id="$(jq -r '.invariant.info.card' "$profile" |
    tr '[:upper:]' '[:lower:]' |
    sed 's/[^a-z0-9]\+/-/g; s/^-//; s/-$//')"
if [[ -z "$camera_id" ]]; then
    gate_fail "could not derive a camera id from $(basename "$profile")"
    gate_finish
fi

# The default is free to sit outside the range [PF:5], so it is clamped rather than trusted.
IFS=$'\t' read -r control control_default control_min control_max <<<"$(writable_control "$profile")"
value="$control_default"
if ((value < control_min)); then value="$control_min"; fi
if ((value > control_max)); then value="$control_max"; fi
gate_note "replaying $(basename "$profile") as cam:$camera_id, naming $control=$value"

# Where the daemon puts its socket, and the words `webcam-handler-cli` refuses a locked store
# with. All three derived from the crates that own them — `schema::paths::APP_DIR`,
# `schema::limits::DAEMON_SOCKET_FILE` and `schema::error::LockProtocol::advice` — because a
# gate that transcribed the refusal would keep asserting a sentence the product had stopped
# saying (docs/9's derived-population rule).
app_dir="$(sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/paths.rs" | head -n1)"
socket_file="$(sed -n 's/^pub const DAEMON_SOCKET_FILE: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/limits.rs" | head -n1)"
lock_advice="$(awk '/LockProtocol::HeldForLifetime => \{/ {
        getline
        sub(/^[[:space:]]*"/, "")
        sub(/"[[:space:]]*$/, "")
        print
        exit
    }' "$root/crates/schema/src/error.rs")"

if [[ -z "$app_dir" || -z "$socket_file" || -z "$lock_advice" ]]; then
    gate_fail "could not read the socket path or D9's lock advice out of the tree (APP_DIR=${app_dir:-?}, DAEMON_SOCKET_FILE=${socket_file:-?}, advice=${lock_advice:-?}); this gate cannot check a refusal it cannot spell"
    gate_finish
fi

# ------------------------------------------------------------------ the programs
#
# From the real checkout, for `json-validates.sh`'s reason: building three binaries inside
# each of the selftest's scratch copies would cost a full compile per case, and the seams
# below replace whatever a case wants replaced.
checkout="$(git rev-parse --show-toplevel)"

# Resolves into $resolved rather than through a command substitution: `gate_fail` inside a
# subshell would be counted by a shell that is about to exit, which is a gate reporting
# nothing while looking like it reported something.
#
# **One name, and that is the owner's ruling rather than a shortcut** (note N90): every
# binary is named after the package it comes from, so `-p`, `--bin` and the path leaf under
# `target/debug/` are one string. Two parameters that are always handed the same value would
# be a second place to be wrong about which of the three this gate meant.
resolved=""
resolve() {
    local override="$1" name="$2"
    resolved="${override:-$checkout/target/debug/$name}"
    if [[ -z "$override" && ! -x "$resolved" ]]; then
        (cd "$checkout" && cargo build --locked --offline -p "$name" --bin "$name" >/dev/null 2>&1) || true
    fi
    if [[ ! -x "$resolved" ]]; then
        gate_fail "$resolved is not an executable program; this gate has nothing to drive"
        gate_finish
    fi
    if [[ -n "$override" ]]; then
        gate_note "driving the $name-shaped program at $resolved"
    fi
}

resolve "${WCH_GATE_WCH:-}" webcam-handler-cli
wch="$resolved"
resolve "${WCH_GATE_WCHC:-}" webcam-handler-client
wchc="$resolved"
resolve "${WCH_GATE_WCHD:-}" webcam-handler-daemon
wchd="$resolved"

# ------------------------------------------------------------------ the fixture
#
# One scratch pair of XDG directories, shared by the daemon and by every `webcam-handler-cli`
# this gate runs — which is the whole point: two processes read one state directory
# concurrently, and the read verbs answering while the daemon holds it is the property the
# header names.
#
# From `gate_socket_scratch_root` and not from `gate_scratch_root`, which is the one place
# that trade is argued: the 2026-08-12 ruling moved test scratch under `target/`, and a Unix
# socket path is capped at 107 bytes, which a directory that deep blows by 17 on this checkout
# alone. Both of this fixture's original reasons still hold at the shorter root — the session
# tree holds sample photos and the repository holds no frames
# (`no-frame-bytes-in-repo.sh`), and a socket the daemon refuses to bind is a client that
# could not name it — and what is new is that a `kill -9` here is reclaimed by
# `gate_scratch_sweep` rather than by nobody.
scratch="$(mktemp -d "$(gate_socket_scratch_root)/wch-cli-parity.XXXXXXXX")"
runtime="$scratch/run"
state="$scratch/state"
mkdir -p "$runtime" "$state"
socket="$runtime/$app_dir/$socket_file"
export XDG_RUNTIME_DIR="$runtime"
export XDG_STATE_HOME="$state"

daemon_pid=""
drain_pid=""
# Invoked by the `trap` below and nowhere else, which shellcheck cannot see. A function
# rather than an inline trap body because it has three things to do in an order and one of
# them must not fail the trap.
# shellcheck disable=SC2329
cleanup() {
    if [[ -n "$drain_pid" ]]; then
        kill "$drain_pid" 2>/dev/null || true
    fi
    if [[ -n "$daemon_pid" ]]; then
        kill "$daemon_pid" 2>/dev/null || true
        wait "$daemon_pid" 2>/dev/null || true
    fi
    rm -rf "$scratch"
    return 0
}
trap cleanup EXIT

# `timeout` is a watchdog and not synchronisation: readiness is a **line the daemon writes**,
# and this bound only turns "a daemon that neither serves nor exits" into a failure.
deadline=180

pipe="$scratch/wchd.stderr"
mkfifo "$pipe"
RUST_LOG=info timeout "$deadline" "$wchd" --backend fake --profile "$profile" >/dev/null 2>"$pipe" &
daemon_pid=$!

serving=0
transcript=""
exec 3<"$pipe"
while IFS= read -r line <&3; do
    transcript+="$line"$'\n'
    if [[ "$line" == *"$socket"* ]]; then
        serving=1
        break
    fi
done
if ((serving == 0)); then
    exec 3<&-
    gate_fail "no daemon is serving $socket, so there is nothing for webcam-handler-client to be compared against; a gate that could not reach a daemon must never read as parity"
    printf '%s' "$transcript" | sed 's/^/        /' >&2
    gate_finish
fi
# Everything it says from here goes nowhere, because a daemon blocked writing into a pipe
# with no reader would be this gate wedging its own subject.
cat <&3 >/dev/null &
drain_pid=$!
exec 3<&-
gate_note "a daemon is serving $socket and holds $state for its lifetime (D9)"

# ------------------------------------------------------------------ the population
#
# Two levels, exact match, both directions. `json-validates.sh`'s header records why an exact
# match: a prefix match once let one `calibrate-start` row answer for a whole seven-verb
# subtree.
help_commands() {
    "$wch" "$@" --help 2>/dev/null |
        awk '/^Commands:/ { inside = 1; next } inside && /^[[:space:]]+[a-z]/ { print $1 } inside && /^$/ { inside = 0 }' |
        grep -v '^help$' || true
}

mapfile -t offered < <(help_commands)
leaves=()
for verb in "${offered[@]}"; do
    mapfile -t subs < <(help_commands "$verb")
    if ((${#subs[@]} == 0)); then
        # A verb with no subcommands is its own leaf, which is the rule as it was.
        leaves+=("$verb")
        continue
    fi
    for sub in "${subs[@]}"; do
        leaves+=("$verb-$sub")
    done
done

if ((${#leaves[@]} == 0)); then
    gate_fail "webcam-handler-cli --help offered no verbs at all; there is no population here to bucket"
    gate_finish
fi

# Every leaf in exactly one bucket.
for leaf in "${leaves[@]}"; do
    matches=0
    for row in "${verbs[@]}"; do
        if [[ "${row%%|*}" == "$leaf" ]]; then
            matches=$((matches + 1))
        fi
    done
    if ((matches == 0)); then
        gate_fail "the surface offers '${leaf/-/ }' and no row above puts it in a bucket; every verb is compared or exempt-with-a-reason, never absent"
    elif ((matches > 1)); then
        gate_fail "'$leaf' appears in $matches rows above; a verb in two buckets is a verb whose treatment nobody can read off the table"
    fi
done

# Every row a leaf, and every bucket in the vocabulary.
for row in "${verbs[@]}"; do
    IFS='|' read -r name bucket _ <<<"$row"
    found=0
    for leaf in "${leaves[@]}"; do
        if [[ "$leaf" == "$name" ]]; then found=1; fi
    done
    if ((found == 0)); then
        gate_fail "a row above names '$name', which webcam-handler-cli --help does not offer; the population is the surface's and not this table's"
    fi
    known=0
    for allowed in "${buckets[@]}"; do
        if [[ "$bucket" == "$allowed" ]]; then known=1; fi
    done
    if ((known == 0)); then
        gate_fail "'$name' is in bucket '$bucket', which is not one of: ${buckets[*]}; an unreadable bucket is how a verb gets exempted without anybody naming a reason"
    fi
done
gate_checked "${#leaves[@]}" "leaf verb(s) scraped from webcam-handler-cli --help at both levels, each required to fall in exactly one named bucket"
gate_require_nonzero "${#leaves[@]}" "leaf verbs"

# ------------------------------------------------------------------ driving them

session=""
compared=0
exempted=0
refusals=0
stamped=0

# Substitute the tokens a row uses. `<snapshot>` and `<session>` are produced by earlier rows,
# so a table reordered under them fails loudly rather than comparing a stale document.
expand() {
    local argv="$1"
    argv="${argv//<camera>/cam:$camera_id}"
    argv="${argv//<control>/$control}"
    argv="${argv//<value>/$value}"
    argv="${argv//<photo>/$scratch/shot.jpg}"
    argv="${argv//<snapshot>/$scratch/snapshot.json}"
    argv="${argv//<session>/$session}"
    printf '%s\n' "$argv"
}

for row in "${verbs[@]}"; do
    IFS='|' read -r name bucket argv <<<"$row"
    argv="$(expand "$argv")"
    if [[ "$argv" == *"<"* ]]; then
        gate_fail "row '$name' still carries an unsubstituted token after expansion ($argv); a row that runs before the row producing its input would compare a document nothing made"
        continue
    fi

    case "$bucket" in
    compared)
        wch_status=0
        # shellcheck disable=SC2086
        mine="$("$wch" --backend fake --profile "$profile" --json $argv 2>"$scratch/wch.err")" || wch_status=$?
        wchc_status=0
        # shellcheck disable=SC2086
        theirs="$("$wchc" --json $argv 2>"$scratch/wchc.err")" || wchc_status=$?

        # Both must *answer*. Without this the gate has a hole big enough to drive a broken
        # fixture through: two programs that both failed would agree on an empty document and
        # be reported as parity.
        if ((wch_status != 0)); then
            gate_fail "webcam-handler-cli --json $argv exited $wch_status: $(head -n1 "$scratch/wch.err")"
            continue
        fi
        if ((wchc_status != 0)); then
            gate_fail "webcam-handler-client --json $argv exited $wchc_status: $(head -n1 "$scratch/wchc.err")"
            continue
        fi
        if ! printf '%s' "$mine" | jq -e . >/dev/null 2>&1; then
            gate_fail "webcam-handler-cli --json $argv did not emit a parseable document, so there is nothing here to compare"
            continue
        fi
        if [[ "$mine" != "$theirs" ]]; then
            gate_fail "webcam-handler-cli and webcam-handler-client do not agree on '${name/-/ }' --json; T4 says a verb exists once and these two renderings have forked"
            diff <(printf '%s\n' "$mine") <(printf '%s\n' "$theirs") | head -n 20 | sed 's/^/        /' >&2
            continue
        fi
        compared=$((compared + 1))
        gate_note "$name → byte-identical --json from both roots ($(printf '%s' "$mine" | wc -c) bytes)"
        ;;

    stamped)
        wch_status=0
        # shellcheck disable=SC2086
        mine="$("$wch" --backend fake --profile "$profile" --json $argv 2>"$scratch/wch.err")" || wch_status=$?
        wchc_status=0
        # shellcheck disable=SC2086
        "$wchc" --json $argv >"$scratch/snapshot.json" 2>"$scratch/wchc.err" || wchc_status=$?
        if ((wch_status != 0 || wchc_status != 0)); then
            gate_fail "'${name/-/ }' did not answer from both roots (webcam-handler-cli $wch_status, webcam-handler-client $wchc_status), so its exemption cannot be measured"
            continue
        fi
        theirs="$(cat "$scratch/snapshot.json")"

        # The exemption, measured to be exactly one field wide: equal without the stamp, and
        # unequal with it. A verb relabelled into this bucket that carries no stamp fails the
        # second half, which is what keeps `stamped` from becoming a place to hide a read verb.
        without_mine="$(printf '%s' "$mine" | jq -S 'del(.taken_at)')"
        without_theirs="$(printf '%s' "$theirs" | jq -S 'del(.taken_at)')"
        if [[ "$without_mine" != "$without_theirs" ]]; then
            gate_fail "'${name/-/ }' differs between the two roots in more than its stamp; the exemption claims one field and this is not it"
            diff <(printf '%s\n' "$without_mine") <(printf '%s\n' "$without_theirs") | head -n 20 | sed 's/^/        /' >&2
            continue
        fi
        if [[ "$mine" == "$theirs" ]]; then
            gate_fail "'${name/-/ }' is byte-identical from both roots, so nothing about it needs exempting; it belongs in 'compared'"
            continue
        fi
        stamped=$((stamped + 1))
        exempted=$((exempted + 1))
        gate_note "$name → exempt, stamped: equal once taken_at is removed, unequal with it"
        ;;

    device | session)
        # The local-only test, and it is a test rather than a repetition of docs/9's claim: a
        # verb `webcam-handler-client` cannot serve would fail here, and that is exactly what a
        # local-only verb is. At P4 there are none, so every exempt row is expected to answer —
        # and one that stops answering names itself.
        # shellcheck disable=SC2086
        if ! "$wchc" --json $argv >"$scratch/exempt.json" 2>"$scratch/wchc.err"; then
            gate_fail "webcam-handler-client cannot answer '${name/-/ }', which webcam-handler-cli offers: $(head -n1 "$scratch/wchc.err"); a verb only one root can run is a local-only verb, and docs/9 requires it named as one rather than met here"
            continue
        fi
        if [[ "$name" == "calibrate-start" ]]; then
            session="$(jq -r '.id // empty' "$scratch/exempt.json" 2>/dev/null || true)"
            if [[ -z "$session" ]]; then
                gate_fail "the session webcam-handler-client opened carries no id, so the compared status row below has nothing to name"
            fi
        fi
        exempted=$((exempted + 1))

        if [[ "$bucket" == session ]]; then
            # The other half of the session exemption, and the half that makes it structural
            # rather than a preference: D9 gives a running daemon the store lock for its
            # lifetime, so `webcam-handler-cli` cannot write this session even if somebody
            # wanted the comparison. Asserted in the refusal's own words, read out of
            # `schema::error`.
            # shellcheck disable=SC2086
            if "$wch" --backend fake --profile "$profile" --json $argv >/dev/null 2>"$scratch/wch.err"; then
                gate_fail "webcam-handler-cli ran '${name/-/ }' while a daemon held $state; D9 gives the daemon that lock for its lifetime and this row's exemption rests on it"
                continue
            fi
            if ! grep -qF "$lock_advice" "$scratch/wch.err"; then
                gate_fail "webcam-handler-cli refused '${name/-/ }' without D9's advice ($lock_advice): $(head -n1 "$scratch/wch.err"); a refusal for some other reason does not establish that the store lock is what stands between these two roots"
                continue
            fi
            refusals=$((refusals + 1))
            gate_note "$name → exempt, session: answered by webcam-handler-client, and refused to webcam-handler-cli by the daemon's lock"
        else
            gate_note "$name → exempt, device: answered by webcam-handler-client, and never driven twice"
        fi
        ;;
    *)
        # Unreachable: the vocabulary check above already failed this row. Here so a future
        # bucket added to the table and not to this loop cannot be silently skipped.
        gate_fail "'$name' is in bucket '$bucket', which this loop does not know how to drive"
        ;;
    esac
done

# The read verbs still answering is the fixture's own load-bearing property, so it is reported
# rather than implied: `webcam-handler-cli` read a state directory the daemon had locked.
gate_checked "$compared" "read verb(s) answered by both roots and compared byte for byte, with webcam-handler-cli reading a state directory the daemon holds"
gate_require_nonzero "$compared" "compared read verbs"
gate_checked "$exempted" "exempt verb(s) driven through webcam-handler-client alone, each required to answer — docs/9's 'no local-only verbs at P4' tested rather than repeated"
gate_require_nonzero "$exempted" "exempt verbs"
gate_checked "$refusals" "session-writing verb(s) refused to webcam-handler-cli in D9's own words while the daemon held the store"
gate_require_nonzero "$refusals" "lock refusals"
gate_checked "$stamped" "stamped verb(s) shown to differ in exactly the one field their exemption names"
gate_require_nonzero "$stamped" "stamped verbs"

gate_finish
