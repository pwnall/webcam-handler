# Both-direction cases for `state-dir-permissions.sh`.
#
# The subject is what a *shipped binary* does to a directory it did not create, so the failing
# arms drive the predicate's documented seam — $WCH_GATE_WCH, the `webcam-handler-cli`-shaped
# program it runs — at programs that get one of its answers wrong each. `pass_case` drives the
# real `webcam-handler-cli`, which is the arm rubric rule 6 requires [S:N10]: a predicate whose
# only input is injectable proves nothing about the repository.
#
# Each stub is a *plausible* wrong tool rather than a nonsense one, and between them they
# drive every assertion the predicate makes except the owner comparison — which needs a second
# account and is accounted for in the predicate's own header. They are shapes this repository
# has actually been in or came within a batch of shipping: the tree created at the ambient
# umask and used (what D9 did until note **N142**), the same tree created and then refused by
# its own creator, the wide tree quietly `chmod`ed instead of refused (what N39 forbids and
# what is the tempting fix), a refusal with no remedy in it, a refusal of the tool's *own*
# directory under a setgid parent (the compatibility break note **N150** repaired, which no
# test in this repository could see), a success reported over a tree nothing was written into,
# and a symlink standing in for the tree (the hole N39 measured in D11's socket directory,
# one directory along).
#
# shellcheck shell=bash

# The directory this tool owns inside `$XDG_STATE_HOME`, from the crate that owns the name —
# the same derivation the predicate makes, because a stub has to lay its tree down where the
# real binary would or the predicate is looking somewhere else.
_app_dir() {
    sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
        "$(gate_root)/crates/schema/src/paths.rs" | head -n1
}

# Write a `webcam-handler-cli`-shaped program that handles `calibrate start` the way the
# arguments say and ignores every other verb.
#
# The shape follows the real one: create the directory if it is missing, **then** check it —
# on the same call, which is where the set-id break fires and why a stub that only checked
# pre-existing directories could not reproduce it.
#
#   $1  where to write the program
#   $2  where the tree goes and how it is created: `private` (mkdir, then `chmod go-rwx`,
#       which is what leaves an inherited set-group bit in place exactly as `mkdir(2)` does),
#       `umask` (left world-traversable, the posture D9 had until note N142), or `symlink`
#       (a private directory made elsewhere and linked to, so every `stat` answers about the
#       target and whoever owns the link decides which target that is)
#   $3  what to do about a directory that fails the check: `refuse` with a runnable remedy,
#       `repair` it quietly, `refuse-silently` with no way out named, `refuse-elsewhere` with
#       a remedy that names a path other than the directory it is refusing, or `accept` —
#       write the session into it and say nothing, which is a tool with no check at all
#   $4  what "fails the check" means: `bits` — anything granted to group or other — or
#       `exact`, a mode word that is not literally 0700, which is the break note N150
#       records
#   $5  what it leaves behind on success: `session` (a session document where the real binary
#       puts one) or `nothing` (an exit 0 over an empty tree)
_stub_cli() {
    local script="$1" create="$2" wide="$3" strictness="$4" leaves="$5" app_dir
    app_dir="$(_app_dir)"
    cat >"$script" <<STUB
#!/usr/bin/env bash
set -euo pipefail
dir="\$XDG_STATE_HOME/$app_dir"
if [[ ! -d "\$dir" ]]; then
    case "$create" in
        private)
            mkdir -p "\$dir"
            chmod go-rwx "\$dir"
            ;;
        symlink)
            mkdir -p "\$XDG_STATE_HOME/elsewhere"
            chmod go-rwx "\$XDG_STATE_HOME/elsewhere"
            ln -s "\$XDG_STATE_HOME/elsewhere" "\$dir"
            ;;
        *)
            mkdir -p "\$dir"
            chmod 0755 "\$dir"
            ;;
    esac
fi
# By path, following whatever the name leads to, which is what a tool that reached for
# \`fs::metadata\` does — and is the whole of the \`symlink\` shape above.
mode="\$(stat -Lc %a "\$dir")"
refuse=0
if (( 8#\${mode: -3} & 8#077 )); then refuse=1; fi
if [[ "$strictness" == exact && "\$mode" != 700 ]]; then refuse=1; fi
if [[ "$wide" == accept ]]; then refuse=0; fi
if (( refuse != 0 )); then
    case "$wide" in
        repair)
            chmod 0700 "\$dir"
            ;;
        refuse-silently)
            printf 'webcam-handler-cli: %s is mode %s\n' "\$dir" "\$mode" >&2
            exit 27
            ;;
        refuse-elsewhere)
            printf 'webcam-handler-cli: %s is mode %s, run \`chmod -R go-rwx %s\` when you have decided that is acceptable\n' \\
                "\$dir" "\$mode" "\$XDG_STATE_HOME/not-the-tree" >&2
            exit 27
            ;;
        *)
            printf 'webcam-handler-cli: %s is mode %s, run \`chmod -R go-rwx %s\` when you have decided that is acceptable\n' \\
                "\$dir" "\$mode" "\$dir" >&2
            exit 27
            ;;
    esac
fi
if [[ "$leaves" == session ]]; then
    mkdir -p "\$dir/sessions/one"
    printf '{}\n' >"\$dir/sessions/one/session.json"
fi
printf 'session one\n'
STUB
    chmod +x "$script"
}

pass_case() {
    "$GATE"
}

# The real binary under a seam that is set rather than defaulted: proves the seam itself does
# not change the verdict, so a failing arm below is the stub's answer and not the plumbing's.
pass_case_the_real_binary_through_the_seam() {
    WCH_GATE_WCH="$(git rev-parse --show-toplevel)/target/debug/webcam-handler-cli" "$GATE"
}

# A tool that creates 0700, refuses a wide tree with a remedy, and asks only about group and
# other — every answer right, through a stub. The arm that separates "this predicate is red on
# a wrong tool" from "this predicate is red on anything that is not the shipped binary".
pass_case_a_stub_that_gets_all_three_right() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-correct.$$"
    _stub_cli "$script" private refuse bits session
    WCH_GATE_WCH="$script" "$GATE"
}

# --------------------------------------------------------------- one wrong tool each

# **The posture D9 had until note N142**, and the arm claim 1's mode assertion did not have:
# the tree created at whatever the umask left — 0775 on this project's own — and then used,
# because a tool with no check is what "created at the umask" means. It is worth the second
# arm below rather than one that does both, since a tool that creates wide and refuses is a
# different wrong tool: this one hands the caller a working session in a directory of
# calibration photographs the whole machine can walk into, and never says a word.
fail_case_a_tool_that_creates_its_tree_at_the_umask_and_writes_a_session_into_it() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-umask.$$"
    _stub_cli "$script" umask accept bits session
    WCH_GATE_WCH="$script" "$GATE"
}

# The same wrong creation with the check bolted on afterwards: it makes the directory 0775,
# looks at what it made, and refuses itself. Every mutating verb fails on a first run, on a
# machine where nothing is wrong except this tool — which is the exit-status half of claim 1,
# and the shape a set-id inheritance produced for real (note N150) one arm further down.
fail_case_a_tool_that_refuses_the_wide_tree_it_created_itself() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-umask-refuse.$$"
    _stub_cli "$script" umask refuse bits session
    WCH_GATE_WCH="$script" "$GATE"
}

# The tempting fix, and the one note N39 forbids: a wide directory silently tightened. An
# operator who is not told cannot act, and what they would have had to act on is however long
# the directory was reachable before the repair.
fail_case_a_tool_that_repairs_a_wide_tree_instead_of_refusing_it() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-repair.$$"
    _stub_cli "$script" private repair bits session
    WCH_GATE_WCH="$script" "$GATE"
}

# A refusal with no way out in it. The tool stops working and the operator is told a mode.
fail_case_a_refusal_that_names_no_remedy() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-silent.$$"
    _stub_cli "$script" private refuse-silently bits session
    WCH_GATE_WCH="$script" "$GATE"
}

# A remedy that runs cleanly and fixes something else — the shape a refusal acquires when the
# message is built from a path that is not the one the check looked at. Running it and trying
# again would loop forever, and the predicate must not paste a command out of a program's
# stderr without knowing what it is about.
fail_case_a_remedy_that_names_a_directory_other_than_the_one_refused() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-elsewhere.$$"
    _stub_cli "$script" private refuse-elsewhere bits session
    WCH_GATE_WCH="$script" "$GATE"
}

# **The compatibility break this gate was written for** (note N150): a check that compares the
# whole mode word refuses the directory it has just created under a setgid parent — a first
# run, on a directory nobody else has ever touched — and the `chmod go-rwx` it prints cannot
# clear a set-id bit, so following the message loops.
fail_case_a_tool_that_refuses_the_setgid_directory_it_just_created() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-setgid.$$"
    _stub_cli "$script" private refuse exact session
    WCH_GATE_WCH="$script" "$GATE"
}

# The non-vacuity arm. A tool that answers "session one" over an empty tree leaves the
# predicate stat-ing a directory nothing was ever written into: the mode would be right and
# would be a fact about nothing. `uds-permissions.sh` carries the same arm for the socket a
# daemon announced and never bound.
fail_case_a_tool_that_reports_a_session_it_never_wrote() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-empty.$$"
    _stub_cli "$script" private refuse bits nothing
    WCH_GATE_WCH="$script" "$GATE"
}

# The link. This tool creates a directory that is genuinely 0700 and genuinely this user's —
# somewhere else — and puts a symlink where its state tree belongs, so `stat` answers about
# the target and everything looks right. Whoever owns the directory the link sits in then
# decides where D9's photographs are written and who owns them, which is how a tree this tool
# created acquires somebody else's owner with nobody changing accounts. Note N39 measured the
# identical hole in the daemon's socket directory.
fail_case_a_tool_that_puts_a_symlink_where_its_state_tree_belongs() {
    local script
    script="$WCH_GATE_SCRATCH/wch-cli-symlink.$$"
    _stub_cli "$script" symlink refuse bits session
    WCH_GATE_WCH="$script" "$GATE"
}
