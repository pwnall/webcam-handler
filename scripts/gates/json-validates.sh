#!/usr/bin/env bash
#
# `--json` output validates against the committed JSON Schema bundle (docs/2 G1).
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

if [[ ! -f "$bundle" ]]; then
    gate_fail "no schema bundle at ${bundle#"$root"/}; 'just generate' writes it"
    gate_finish
fi

# The read verbs and the `$defs` name each one's answer must validate against.
# `<profile>` is substituted with a committed profile path.
verbs=(
    "list|CameraList|list"
    "info|CameraDetail|info cam:"
    "controls|ControlReport|controls cam:"
    "profile-capture|DeviceProfile|profile capture cam:"
)

# Sorted, so the gate examines the same profile on every run and in every scratch copy —
# `find` has no defined order, and a gate whose subject varies run to run is a gate whose
# green means something different each time.
profile="$(gate_find "$root/corpus/profiles" -name '*.json' | tr '\0' '\n' | sort | head -n1)"
if [[ -z "$profile" ]]; then
    gate_fail "corpus/profiles/ is empty; this gate replays a committed profile so its answers do not depend on attached hardware"
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
    checked=$((checked + 1))

    # `cam:` rows get the derived id appended; `list` takes no camera.
    case "$argv" in
    *"cam:") argv="${argv}${camera_id}" ;;
    esac

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
    # this gate exists for, and the recorded limit below says what it does not catch.
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
    gate_note "$name → #/\$defs/$def"
done

gate_checked "$checked" "--json verb answers validated against the committed bundle"
gate_require_nonzero "$checked" "--json verb answers"

# Every verb the CLI offers must have a row above. Derived from `--help`, so a verb added
# without a row is a failure rather than a quiet omission.
mapfile -t offered < <("$binary" --help 2>/dev/null |
    awk '/^Commands:/ { inside = 1; next } inside && /^[[:space:]]+[a-z]/ { print $1 } inside && /^$/ { inside = 0 }' |
    grep -v '^help$' || true)

for verb in "${offered[@]}"; do
    found=0
    for row in "${verbs[@]}"; do
        case "${row%%|*}" in
        "$verb" | "$verb"-*) found=1 ;;
        esac
    done
    if ((found == 0)); then
        gate_fail "the CLI offers '$verb' but no row above validates its --json answer"
    fi
done
gate_checked "${#offered[@]}" "CLI verb(s) checked for a validation row"
gate_require_nonzero "${#offered[@]}" "CLI verbs"

gate_finish
