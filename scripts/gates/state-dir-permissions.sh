#!/usr/bin/env bash
#
# D9's session tree is private, and the refusal that keeps it private is one an operator can
# get out of — design D9, note **N39**, notes **N142** and **N150**, AGENTS.md "A frame may
# contain a person".
#
# A calibration session is the one place in this tool where frames come to rest:
# `photos/<control>/<pass>/<value>.jpg` is a photograph of whatever the camera was pointed
# at, and the document beside it names the camera, the task and the operator's goal in their
# own words. N142 gave that tree the posture D11's runtime directory has had since P4b —
# created 0700, and a wider mode **refused rather than repaired**, because a silent `chmod`
# would hide that the directory had been reachable and for however long it was, it may
# already have been read.
#
# ## Why a shell predicate, when the mode itself is a Rust test
#
# `crates/engine/src/store.rs` holds the unit half, both directions, and it is the better
# place for the *rule*. What it cannot reach is the **shape every existing operator is in**:
# a state directory that already exists, at a mode an older build left, met by a *shipped
# binary* on its next run. Every Rust test roots its store at a directory the tool creates
# itself (`TempStore` → `paths::scratch_dir`, 0700 by construction), and every other shell
# gate points `$XDG_STATE_HOME` at a fresh directory, so the checked root is always one this
# build made. Nothing in the suite planted a pre-existing tree and drove a real
# `webcam-handler-cli` at it — which is how a compatibility break that refuses every mutating
# verb, permanently, came within a batch of shipping with no coverage at all.
#
# So this gate asks the three questions a migration has to answer:
#
#   1. a tree this build creates is private, and owned by whoever ran it;
#   2. a **pre-existing** world-traversable tree is refused, left alone, and the refusal
#      names the remedy — *and the remedy works*, which is the half a refusal usually gets
#      away with not proving;
#   3. a tree this build creates under a **setgid** parent is not refused. Linux inherits
#      `S_ISGID` on `mkdir`, so on a group-managed home the tool creates 2700; a check that
#      asked "is the mode exactly 0700?" refused its own directory on the same call, on a
#      first run, and printed a `chmod go-rwx` that by POSIX cannot clear a set-id bit.
#
# ## What it drives, and the seam
#
# A real `webcam-handler-cli` against the fake backend replaying a committed profile, running
# `calibrate start` — a mutating verb, so it passes through `StoreLock::acquire`, which is the
# one funnel the check lives on. $WCH_GATE_WCH is the documented seam: the selftest points it
# at programs that get one of the three answers wrong each, and `pass_case` always drives the
# real binary, which is rubric rule 6's requirement that one arm runs the real tool [S:N10].
#
# ## Which of these assertions an arm actually drives red
#
# Every one of them except the owner comparison, and that exception is named rather than left
# for a reader to discover. Claim 1 landed with four assertions and one arm — the arm created
# its tree wide and *then refused it*, so the predicate went red on the exit status and the
# mode line below was never reached (the shape B2c repaired). The mode, the symlink and the
# "a session was actually written" lines each have their own wrong tool in the case file now.
#
# The owner line does not, and cannot through this seam: a stub runs as whoever runs the
# selftest, so every directory it creates comes back owned by them. Driving it needs a second
# account and `sudo -n`, which is the cost `uds-permissions.sh` pays once for D11's directory
# (note N44) and `gate_skip`s where the host cannot. What stands in for it here is the pair
# that can be driven — `engine::store`'s `a_private_state_directory_somebody_else_owns_is_
# refused_naming_both_uids`, which is red on the euid *as a value* for exactly this reason
# (note N150), and the symlink line above, which is how a tree this tool created comes to
# have somebody else's owner without anybody changing accounts.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The directory this tool owns inside `$XDG_STATE_HOME`, read out of the crate that owns the
# name rather than transcribed: a rename must move this gate with it (docs/9's derived-
# population rule).
app_dir="$(sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/paths.rs" | head -n1)"
if [[ -z "$app_dir" ]]; then
    gate_fail "could not read APP_DIR out of crates/schema/src/paths.rs; this gate cannot find the directory it must check"
    gate_finish
fi

# The mode, written here as *policy*. Deriving it from `engine::store::STATE_DIR_MODE` would
# produce a gate that follows the code it checks — a build that widened the constant would
# widen the gate with it and stay green. This is the second opinion.
wanted_mode=700

profile=""
for candidate in $(gate_find "$root/corpus/profiles" -name '*.json' | tr '\0' '\n' | sort); do
    profile="$candidate"
    break
done
if [[ -z "$profile" ]]; then
    gate_fail "no committed device profile to replay; this gate has no camera to start a session against"
    gate_finish
fi

camera_id="$(sed -n 's/.*"card"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$profile" |
    head -n1 |
    tr '[:upper:]' '[:lower:]' |
    sed 's/[^a-z0-9]\+/-/g; s/^-//; s/-$//')"
if [[ -z "$camera_id" ]]; then
    gate_fail "could not derive a camera id from $(basename "$profile")"
    gate_finish
fi
gate_note "replaying $(basename "$profile") as cam:$camera_id"

# The binary comes from the real checkout for `cli-parity.sh`'s reason: building it inside
# each of the selftest's scratch copies would cost a full compile per case, and the seam above
# replaces the program wholesale when a case wants a different one.
checkout="$(git rev-parse --show-toplevel)"
wch="${WCH_GATE_WCH:-$checkout/target/debug/webcam-handler-cli}"
if [[ -n "${WCH_GATE_WCH:-}" ]]; then
    gate_note "driving the webcam-handler-cli-shaped program at $wch (WCH_GATE_WCH)"
elif [[ ! -x "$wch" ]]; then
    (cd "$checkout" && cargo build --locked --offline -p webcam-handler-cli --bin webcam-handler-cli >/dev/null 2>&1) ||
        {
            gate_fail "could not build webcam-handler-cli; this gate has nothing to drive"
            gate_finish
        }
fi
if [[ ! -x "$wch" ]]; then
    gate_fail "$wch is not an executable program"
    gate_finish
fi

scratch="$(mktemp -d "$(gate_scratch_root)/wch-state-dir.XXXXXXXX")"
trap 'chmod -R u+rwX "$scratch" 2>/dev/null || true; rm -rf "$scratch"' EXIT

transcript=""
status=0

# Start a session against `$1` as `$XDG_STATE_HOME`. Never fails the script: the answer is
# the subject.
start_session() {
    local state="$1" task="$2"
    status=0
    transcript="$(XDG_STATE_HOME="$state" "$wch" --backend fake --profile "$profile" \
        calibrate start "cam:$camera_id" --task "$task" --goal 'gate-run' 2>&1)" || status=$?
}

# ------------------------------------------------------------------ claim 1
#
# A tree this build creates for itself is private, and ours.

fresh="$scratch/fresh"
mkdir -p "$fresh"
chmod 0755 "$fresh"
start_session "$fresh" fresh

created=0
if ((status != 0)); then
    gate_fail "a shipped webcam-handler-cli could not start a session under a state directory it created itself (exit $status)"
    printf '%s\n' "$transcript" | sed 's/^/        /' >&2
else
    mode="$(stat -c %a "$fresh/$app_dir" 2>/dev/null || true)"
    owner="$(stat -c %U "$fresh/$app_dir" 2>/dev/null || true)"
    if [[ "$mode" != "$wanted_mode" ]]; then
        gate_fail "$fresh/$app_dir was created at mode ${mode:-<missing>}; D9's session tree holds calibration photographs and must be $wanted_mode"
    fi
    if [[ "$owner" != "$(id -un)" ]]; then
        gate_fail "$fresh/$app_dir is owned by ${owner:-<missing>}, not $(id -un)"
    fi
    # **The object, not the name.** Rust's `fs::metadata` follows links, so a tool whose
    # state tree is a symlink checks whatever the link points at — a directory that may be
    # 0700 and somebody else's, and that whoever owns the link can re-point between one verb
    # and the next. That is how a tree "this build created" acquires an owner the comparison
    # above cannot catch, which matters here because no arm of this gate can hand the
    # predicate a directory belonging to a second account. `uds-permissions.sh` asks the same
    # question of D11's socket directory (note N39), where the answer is enforced by the
    # daemon as well as asserted.
    if [[ -L "$fresh/$app_dir" ]]; then
        gate_fail "$fresh/$app_dir is a symlink; D9's tree is the directory itself, and the mode a link's target has today is not one this tool decides"
    fi
    # Non-vacuity: a mode is only evidence if the tool really put a session under it.
    if [[ -z "$(gate_find "$fresh/$app_dir" -name 'session.json' | tr -d '\0')" ]]; then
        gate_fail "no session document was written under $fresh/$app_dir, so the mode checked above is a mode on an empty directory"
    fi
    created=1
    gate_note "a fresh state tree came out at mode $mode, owner $owner"
fi
gate_checked "$created" "state tree(s) a shipped webcam-handler-cli created, checked for mode $wanted_mode, owner, not a link, and a session actually written"
gate_require_nonzero "$created" "created state trees"

# ------------------------------------------------------------------ claim 2
#
# A pre-existing world-traversable tree — what an operator who ran an earlier build has — is
# refused rather than repaired, and the way out that the refusal names is a way out.

legacy="$scratch/legacy"
mkdir -p "$legacy/$app_dir/sessions"
chmod -R 0755 "$legacy"
start_session "$legacy" legacy

if ((status == 0)); then
    gate_fail "a shipped webcam-handler-cli wrote a calibration session into $legacy/$app_dir, which is mode 0755; a directory holding photographs that the whole machine can walk into must be a refusal"
fi
left="$(stat -c %a "$legacy/$app_dir" 2>/dev/null || true)"
if [[ "$left" != "755" ]]; then
    gate_fail "the tool changed $legacy/$app_dir from 0755 to ${left:-<missing>}; a wrong mode is refused rather than repaired, because a silent chmod hides that the directory was reachable (note N39)"
fi
if ! printf '%s' "$transcript" | grep -q 'chmod'; then
    gate_fail "the refusal over a world-traversable state directory names no way out: $transcript"
fi

# **The remedy, actually run.** A refusal an operator cannot act on is a tool that has
# stopped working, and this is the only place in the suite where the printed command meets
# the directory it is printed about.
# The single quotes are the point and shellcheck cannot know it: the backticks in the
# expression are the ones the *tool's message* puts around its remedy, matched literally.
# shellcheck disable=SC2016
remedy="$(printf '%s' "$transcript" | sed -n 's/.*run `\(chmod[^`]*\)`.*/\1/p' | head -n1)"
if [[ -z "$remedy" ]]; then
    gate_fail "the refusal does not carry a runnable chmod for this gate to try: $transcript"
    gate_finish
fi
# **Split into an argv rather than handed back to a shell.** `eval` on a string parsed out of
# a program's own stderr is a code-execution seam whatever the program is: today every path
# here comes from `mktemp` and cannot carry a metacharacter, but a gate that runs whatever the
# tool prints is one directory name away from running something else, and this suite does not
# get to hold "unreachable today" as a security argument. `read -a` splits on whitespace and
# on nothing else, so what executes is the command the tool named with the arguments it named
# — no expansion, no substitution, no second command after a `;`.
#
# The consequence is that a path containing whitespace would split into two arguments and be
# caught by the check below rather than run; that is the right way round, because a remedy a
# person could not paste into a shell either is not a remedy.
read -r -a remedy_argv <<<"$remedy"
if [[ "${remedy_argv[0]}" != "chmod" || "${remedy_argv[-1]}" != "$legacy/$app_dir" ]]; then
    gate_fail "the refusal's way out is not a chmod of the directory it refused ($remedy); an operator following it would be fixing something else"
    gate_finish
fi
gate_note "running the remedy the tool printed: $remedy"
"${remedy_argv[@]}" || gate_fail "the remedy the refusal printed does not run: $remedy"

start_session "$legacy" legacy
if ((status != 0)); then
    gate_fail "after running the remedy the tool itself printed, it still refuses $legacy/$app_dir (exit $status); an operator following the message would loop"
    printf '%s\n' "$transcript" | sed 's/^/        /' >&2
fi
gate_checked 1 "pre-existing world-traversable state tree, refused unchanged with a named remedy that then works"

# ------------------------------------------------------------------ claim 3
#
# A tree this build creates under a setgid parent. Linux inherits `S_ISGID` on `mkdir`, so
# the directory arrives at 2700 — and a check that compared the whole mode word refused its
# own first run, forever, with a remedy that by POSIX cannot clear a set-id bit.

inherited="$scratch/group-managed"
mkdir -p "$inherited"
chmod 2755 "$inherited"
start_session "$inherited" inherited

if [[ "$(stat -c %a "$inherited/$app_dir" 2>/dev/null || true)" != 2* ]]; then
    # Not a failure: the bit is a property of the filesystem under the scratch root, and a
    # host that does not propagate it has nothing here to have got wrong. Counted and named,
    # because a silent pass is the thing this suite exists not to do.
    gate_skip 1 "the filesystem under $scratch did not inherit the set-group bit, so nothing here was arranged; the mode checks above still ran"
elif ((status != 0)); then
    gate_fail "a shipped webcam-handler-cli refused $inherited/$app_dir, a directory it had just created itself under a setgid parent (exit $status); the set-group bit is inherited by mkdir and is not an exposure"
    printf '%s\n' "$transcript" | sed 's/^/        /' >&2
else
    perms="$(stat -c %a "$inherited/$app_dir")"
    if [[ "${perms: -3}" != "$wanted_mode" ]]; then
        gate_fail "$inherited/$app_dir came out at mode $perms; the set-group bit is inherited, the permission bits are still ours to get right"
    fi
    gate_checked 1 "state tree created under a setgid parent, accepted at mode $perms rather than refused on its own first run"
fi

gate_finish
