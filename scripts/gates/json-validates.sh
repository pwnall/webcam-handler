#!/usr/bin/env bash
#
# `--json` output validates against the committed JSON Schema bundle (docs/7 G1).
#
# `schema-artifacts-current.sh` proves the bundle still describes the Rust *types*. This
# proves the other half: that what the binary actually prints is one of those types. The
# two together are what makes "`--json` emits schema DTOs verbatim" (design §2.7) a
# checkable claim rather than a convention — a renderer that wrapped the answer in an
# envelope, added a timestamp, or emitted a hand-built object would satisfy the first
# check and fail this one.
#
# **Run against the fake backend, deliberately.** The shape of `wch list --json` must not
# depend on what is plugged in, and a gate that needed a camera could not run in CI. The
# committed corpus supplies the devices; `wch --backend fake --profile …` replays them.
#
# The verb-to-type mapping below is the one thing here that is written down rather than
# derived: a Rust trait does not reify its methods, and nothing in the bundle records
# which verb returns which type. It is kept honest by the count — every verb the CLI's
# help lists must appear, and a verb added without a row fails.
#
# **The binary comes from the real checkout, the bundle and the corpus from the tree under
# test.** That asymmetry is deliberate and it is the seam the selftest drives: building
# `wch` inside each of the selftest's scratch copies would cost a full compile per case,
# and this predicate's subject is the *document* — whether what the binary prints is one of
# the bundle's types. So the failing arms move the schema and the replayed profile, which
# is where the interesting inverses live. What that does not catch is a source change that
# alters the emitted shape without a rebuild; `just ci` builds before it runs the gates, so
# in the pipeline the binary is current, and this line is the recorded limit rather than an
# unstated assumption.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
bundle="$root/schemas/webcam-handler-schema.json"

# The slug of a control this gate may write: an integer, writable, and holding a value
# inside its own declared range. Read out of the profile rather than transcribed, so a
# re-capture that renames a control does not silently break the write rows.
#
# `0x0004` is READ_ONLY and `0x0001` is DISABLED; both make a control unwritable, and both
# are checked against the raw flag word because that is what the profile records.
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

if [[ ! -f "$bundle" ]]; then
    gate_fail "no schema bundle at ${bundle#"$root"/}; 'just generate' writes it"
    gate_finish
fi

# Every verb and the `$defs` name its answer must validate against. Three tokens are
# substituted into the argv: `<camera>` anywhere in the line, `<control>` with a writable
# control the replayed profile actually has, and `<snapshot>` with a document this gate
# produces by running `snapshot` first.
#
# `photo` takes a `-o` under a temporary directory: `--json` requires one (the bytes and
# the document cannot share standard output), and writing outside the tree is what keeps
# `no-frame-bytes-in-repo.sh` true even though these frames are synthetic.
# The `calibrate` rows are **ordered**: they are one session, driven from `start` to
# `apply`, because a calibration verb's answer only exists once the verbs before it have
# run. That is not a weakness of the check — it is what makes the documents real. The
# sweep takes one value with the settle disabled, because this gate is about the shape of
# the answer and a ten-frame settle per sample would buy nothing.
verbs=(
    "list|CameraList|list"
    "info|CameraDetail|info <camera>"
    "controls|ControlReport|controls <camera>"
    "get|ControlDesc|get <camera> <control>"
    "set|WriteReport|set <camera> <control>=<value>"
    "snapshot|Snapshot|snapshot <camera>"
    "restore|RestoreReport|restore <camera> <snapshot>"
    "photo|PhotoReport|photo <camera> -o <photo>"
    "calibrate-start|Session|calibrate start <camera> --task gate --goal gate-run"
    "calibrate-plan|Session|calibrate plan <camera> --task gate <control>"
    "calibrate-sweep|Session|calibrate sweep <camera> --task gate <control> --values <value> --skip-frames 0"
    "calibrate-status|SessionStatus|calibrate status <camera> --task gate"
    "calibrate-select|Session|calibrate select <camera> --task gate <control> --metric sharpness"
    "calibrate-apply|WriteReport|calibrate apply <camera> --task gate"
    "calibrate-restore|RestoreReport|calibrate restore <camera> --task gate"
    "calibrate-list|SessionList|calibrate list <camera>"
    "profile-capture|DeviceProfile|profile capture <camera>"
)

# Sorted, so the gate examines the same profile on every run and in every scratch copy —
# `find` has no defined order, and a gate whose subject varies run to run is a gate whose
# green means something different each time.
#
# The *first sorted profile with a writable integer control*, rather than simply the first:
# `get` and `set` need a control to name, and the corpus's first entry alphabetically
# (`chicony-ir`) exposes three controls, none of them writable. Still deterministic, and
# still derived from the tree rather than transcribed.
profile=""
for candidate in $(gate_find "$root/corpus/profiles" -name '*.json' | tr '\0' '\n' | sort); do
    if [[ -n "$(writable_control "$candidate")" ]]; then
        profile="$candidate"
        break
    fi
done
if [[ -z "$profile" ]]; then
    gate_fail "no committed profile exposes a writable integer control, so the write verbs cannot be exercised against a replayed device"
    gate_finish
fi

# The camera id the replayed profile enumerates as. Read from the profile rather than
# transcribed, so a re-capture with a different card name does not silently break this.
camera_id="$(jq -r '.invariant.info.card' "$profile" |
    tr '[:upper:]' '[:lower:]' |
    sed 's/[^a-z0-9]\+/-/g; s/^-//; s/-$//')"
if [[ -z "$camera_id" ]]; then
    gate_fail "could not derive a camera id from $(basename "$profile")"
    gate_finish
fi
gate_note "replaying $(basename "$profile") as cam:$camera_id"

# The control the write rows name, and a value inside its declared range. The *default* is
# free to sit outside the range [PF:5], so it is clamped rather than trusted — a gate that
# fed a device an out-of-range value would be testing the clamp instead of the schema.
IFS=$'\t' read -r control control_default control_min control_max <<<"$(writable_control "$profile")"
value="$control_default"
if ((value < control_min)); then value="$control_min"; fi
if ((value > control_max)); then value="$control_max"; fi
gate_note "writing $control=$value (declared range $control_min..$control_max)"

# Scratch space for the documents and bytes these rows produce. Outside the tree, so
# `no-frame-bytes-in-repo.sh` stays true and a failed run leaves nothing behind.
scratch="$(mktemp -d "${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}/wch-json-validates.XXXXXXXX")"
trap 'rm -rf "$scratch"' EXIT

# The calibration verbs write a session tree (D9), and it goes in the scratch directory
# with everything else: sample photos are frames, the repository holds no frames
# (`no-frame-bytes-in-repo.sh`), and a gate that wrote into the operator's real state
# directory would put a session called "gate" in front of them every time CI ran.
export XDG_STATE_HOME="$scratch/state"

# The real checkout, whatever tree is under test — see the note above.
checkout="$(git rev-parse --show-toplevel)"
binary="$checkout/target/debug/wch"
if [[ ! -x "$binary" ]]; then
    (cd "$checkout" && cargo build --locked --offline -p webcam-handler-cli --bin wch >/dev/null 2>&1) ||
        {
            gate_fail "could not build wch"
            gate_finish
        }
fi

checked=0
for row in "${verbs[@]}"; do
    IFS='|' read -r name def argv <<<"$row"

    argv="${argv//<camera>/cam:$camera_id}"
    argv="${argv//<control>/$control}"
    argv="${argv//<value>/$value}"
    argv="${argv//<photo>/$scratch/shot.jpg}"
    if [[ "$argv" == *"<snapshot>"* ]]; then
        # `restore` needs a document, and the only honest source of one is `snapshot`
        # itself: a hand-written fixture would validate a shape nothing produces.
        if ! "$binary" --backend fake --profile "$profile" --json \
            snapshot "cam:$camera_id" >"$scratch/snapshot.json" 2>/dev/null; then
            gate_fail "could not take the snapshot the restore row replays"
            continue
        fi
        argv="${argv//<snapshot>/$scratch/snapshot.json}"
    fi

    output=""
    status=0
    # shellcheck disable=SC2086
    output="$("$binary" --backend fake --profile "$profile" --json $argv 2>/dev/null)" || status=$?
    if ((status != 0)); then
        gate_fail "wch --json $argv exited $status; a verb that cannot answer cannot be validated"
        continue
    fi

    if ! printf '%s' "$output" | jq -e . >/dev/null 2>&1; then
        gate_fail "wch --json $argv did not emit parseable JSON"
        continue
    fi

    # The bundle keeps every type under `$defs`, so validation is: does the document match
    # `#/$defs/<name>`? Without a JSON Schema validator in the offline toolchain, the
    # checkable core is enforced directly — every required property present, and no
    # property the schema does not declare. That catches the envelope-and-timestamp defect
    # this gate exists for; types, formats, nested shapes and array element schemas go
    # unchecked, and docs/9's recorded-limits section says so.
    if ! jq -e --slurpfile doc <(printf '%s' "$output") --arg def "$name" '
        .["$defs"][$ARGS.named.d] as $schema
        | ($doc[0]) as $value
        | if $schema == null then false
          else
            (($schema.required // []) | all(. as $k | ($value | has($k))))
            and
            (if ($schema.properties // null) == null then true
             else ($value | keys) | all(. as $k | ($schema.properties | has($k)))
             end)
          end
    ' --arg d "$def" "$bundle" >/dev/null 2>&1; then
        gate_fail "wch --json $argv does not match #/\$defs/$def in the committed bundle"
        continue
    fi
    # Counted here, at the end, and not on entry: a row that failed to answer or failed
    # to validate was *attempted*, not validated, and `gate_checked` is the number the
    # report stands on.
    checked=$((checked + 1))
    gate_note "$name → #/\$defs/$def"
done

gate_checked "$checked" "--json verb answers validated against the committed bundle"
gate_require_nonzero "$checked" "--json verb answers"

# Every verb the CLI offers must have a row above. Derived from `--help`, so a verb added
# without a row is a failure rather than a quiet omission — and derived at **both levels**,
# because two of the ten top-level names are subtrees.
#
# The single-level version of this loop was the P3 review's finding: it scraped only
# `wch --help`, and its membership test accepted a top-level verb if *any* row's name began
# with it, so one `calibrate-start` row satisfied the whole seven-verb `calibrate` subtree.
# Deleting the other six rows left the gate green while it validated six fewer documents —
# the criterion in `phase-criteria.tsv`, docs/7 §P3d and docs/9 all assert a property the
# predicate did not have. Before P3 the gap was latent (`profile` had one subcommand); P3d
# made `calibrate` a seven-verb tree and made it load-bearing. Note N10's family, again:
# a gate green while checking less than it claims.
help_commands() {
    "$binary" "$@" --help 2>/dev/null |
        awk '/^Commands:/ { inside = 1; next } inside && /^[[:space:]]+[a-z]/ { print $1 } inside && /^$/ { inside = 0 }' |
        grep -v '^help$' || true
}

# Exact match, not a prefix: `calibrate-start` no longer answers for `calibrate-sweep`.
has_row() {
    local want="$1" row
    for row in "${verbs[@]}"; do
        if [[ "${row%%|*}" == "$want" ]]; then
            return 0
        fi
    done
    return 1
}

mapfile -t offered < <(help_commands)

leaves=0
for verb in "${offered[@]}"; do
    mapfile -t subs < <(help_commands "$verb")
    if ((${#subs[@]} == 0)); then
        # A verb with no subcommands is its own leaf, which is the rule as it was.
        leaves=$((leaves + 1))
        if ! has_row "$verb"; then
            gate_fail "the CLI offers '$verb' but no row above validates its --json answer"
        fi
        continue
    fi
    for sub in "${subs[@]}"; do
        leaves=$((leaves + 1))
        if ! has_row "$verb-$sub"; then
            gate_fail "the CLI offers '$verb $sub' but no row named '$verb-$sub' validates its --json answer"
        fi
    done
done
gate_checked "$leaves" "CLI verb(s), subcommands included, checked for a validation row"
gate_require_nonzero "$leaves" "CLI verbs"

gate_finish
