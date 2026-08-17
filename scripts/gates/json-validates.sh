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
# **Run against the fake backend, deliberately.** The shape of `webcam-handler-cli list --json`
# must not depend on what is plugged in, and a gate that needed a camera could not run in CI.
# The committed corpus supplies the devices; `webcam-handler-cli --backend fake --profile …`
# replays them.
#
# **The verb-to-type mapping is no longer written down here.** It used to be the one thing
# this predicate transcribed — a Rust trait does not reify its methods, and nothing in the
# bundle records which verb returns which type — and docs/7 P6e is what moved it: the
# generated agent guide teaches the same mapping to an unattended caller, and a second
# hand-written copy of it is precisely the drift that sub-milestone exists to prevent (note
# **N122**). It lives in `crates/cli-core/json-contracts.tsv` now, beside the surface it is a
# fact about, with three readers: `cli_core::contracts` (whose tests prove the rows and the
# clap tree name the same verbs, in both directions), `webcam-handler-xtask` (which prints it
# into the guide), and this gate.
#
# What stays here is the *argv* per verb — how to make each one answer, cheaply, over a
# replayed device. That is this gate's business and nobody else's: `--skip-frames 0` on the
# sweep row buys a fast gate and would be bad advice in a manual. The two tables are checked
# against the same authority from opposite ends — this one against `--help`, that one against
# the clap tree — so neither can quietly shrink.
#
# ## The failure document is a `--json` answer too (owner ruling, 2026-08-15; note **N127**)
#
# A `--json` invocation that fails prints `schema::error::Failure` on standard output, so
# "`--json` emits schema DTOs verbatim" is now a claim about failing runs as well and this
# predicate makes it one: two refusals are driven, each validated against `#/$defs/Failure` by
# the same jq the answers go through. Note **N124** measured the behaviour this replaced —
# nothing at all on standard output, `--json` or not — so a gate that only ever drove verbs
# that answer would have been green throughout the defect's whole life.
#
# **And the marker is checked from both sides.** A failure is told from an answer by one
# property name, `schema::error::FAILURE_MARKER`, which this gate reads out of the tree rather
# than transcribing — `cli-parity.sh` reads D9's lock advice the same way, for the same reason:
# a gate carrying its own copy keeps looking for a marker the product has stopped emitting.
# Every answering verb above is required *not* to carry it, and every refusal below is required
# to. `webcam-handler-xtask`'s
# `no_document_a_verb_answers_with_can_be_mistaken_for_the_failure_document` asks the same
# question of the committed *shapes*; this asks it of the bytes a binary printed.
#
# **The binary comes from the real checkout, the bundle and the corpus from the tree under
# test.** That asymmetry is deliberate and it is the seam the selftest drives: building
# `webcam-handler-cli` inside each of the selftest's scratch copies would cost a full compile
# per case, and this predicate's subject is the *document* — whether what the binary prints is
# one of the bundle's types. So the failing arms move the schema and the replayed profile,
# which is where the interesting inverses live. What that does not catch is a source change
# that alters the emitted shape without a rebuild; `just ci` builds before it runs the gates,
# so in the pipeline the binary is current, and this line is the recorded limit rather than an
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

# The property name that tells a failure document from an answer, read out of the crate that
# owns it. See the header for why this is derived rather than written here.
marker="$(sed -n 's/^pub const FAILURE_MARKER: &str = "\([^"]*\)".*/\1/p' \
    "$root/crates/schema/src/error.rs" | head -n1)"
if [[ -z "$marker" ]]; then
    gate_fail "webcam-handler-schema no longer declares FAILURE_MARKER; this gate reads the name that distinguishes a --json failure from a --json answer out of the tree rather than repeating it, and cannot check a marker it cannot spell"
    gate_finish
fi
gate_note "a --json failure is marked by the '$marker' property"

# Every D13 kind's wire spelling, from the table `crates/api` pins its codes against — which is
# generated from `ErrorKind::ALL` and checked against it in both directions by that crate's own
# tests. A refusal naming a kind outside this set is a document describing a registry this
# build does not have.
mapfile -t d13_kinds < <(awk -F'\t' '/^#/ { next } NF >= 2 { print $1 }' \
    "$root/crates/api/fixtures/d13-rpc-codes.tsv")
if ((${#d13_kinds[@]} == 0)); then
    gate_fail "could not read the D13 kind spellings out of crates/api/fixtures/d13-rpc-codes.tsv; a refusal's discriminant would then be checked against nothing"
    gate_finish
fi

# One document against one `$defs` entry. The bundle keeps every type under `$defs`, so
# validation is: does the document match `#/$defs/<name>`? Without a JSON Schema validator in
# the offline toolchain, the checkable core is enforced directly — every required property
# present, and no property the schema does not declare. That catches the
# envelope-and-timestamp defect this gate exists for; types, formats, nested shapes and array
# element schemas go unchecked, and docs/9's recorded-limits section says so.
#
# A function since P6f, because the failure rows below validate exactly as the answers do and a
# second copy of this jq would be a second opinion about what "matches the bundle" means.
#
# ## It answers with a reason and not with a boolean, since 2026-08-17
#
# The three ways a document can fail this are three different things to do, and until L25's second
# tranche they printed one sentence: *"… does not match #/$defs/X in the committed bundle"*, with
# nothing in it to say which. Two of the case file's arms — a bundle that requires a property the
# answer has not got, and an answer carrying a property the bundle does not declare — were
# therefore indistinguishable to the harness, which is note **N242**'s finding one predicate along
# and is recorded as note **N247**.
#
# The direction matters to a reader more than it does to the selftest. *Missing a required
# property* is the **document** falling behind the schema: a serializer stopped emitting a field
# somebody still declares. *Carrying an undeclared one* is the **schema** falling behind the
# document, which on this tree has one overwhelmingly likely cause — a doc comment or a type
# changed and `just generate` was not run, which `schema-artifacts-current.sh` says in its own
# words. Telling an author "run `just generate`" and telling them "your `--json` answer lost a
# field" are not the same message.
#
# Empty output means it validated; anything else is the reason it did not. Printed rather than
# returned, because a `gate_fail` inside a command substitution is a violation that prints and
# does not count — the trap `oracle-rung-accounting.sh` names beside its own `SHAPE` variable.
why_it_does_not_validate() {
    local document="$1" def="$2"
    jq -r --slurpfile doc <(printf '%s' "$document") '
        .["$defs"][$ARGS.named.d] as $schema
        | ($doc[0]) as $value
        | if $schema == null then "the bundle defines no such type"
          else
            (($schema.required // []) - ($value | keys)) as $missing
            | (if ($schema.properties // null) == null then []
               else ($value | keys) - ($schema.properties | keys)
               end) as $undeclared
            | if ($missing | length) > 0 then
                "the answer carries no \($missing | join(", ")), which the bundle requires"
              elif ($undeclared | length) > 0 then
                "the answer carries \($undeclared | join(", ")), which the bundle does not declare"
              else "" end
          end
    ' --arg d "$def" "$bundle" 2>/dev/null
}

# Every verb and the `$defs` name its answer must validate against. Three tokens are
# substituted into the argv: `<camera>` anywhere in the line, `<control>` with a writable
# control the replayed profile actually has, and `<snapshot>` with a document this gate
# produces by running `snapshot` first.
#
# `photo` takes a `-o` under a temporary directory: `--json` requires one (the bytes and
# the document cannot share standard output), and writing where no tree walk looks is what
# keeps `no-frame-bytes-in-repo.sh` true even though these frames are synthetic. Since the
# 2026-08-12 ruling that is under `target/`, which `gate_find` prunes, rather than outside
# the worktree (note N84).
# The `calibrate` rows are **ordered**: they are one session, driven from `start` to
# `apply`, because a calibration verb's answer only exists once the verbs before it have
# run. That is not a weakness of the check — it is what makes the documents real. The
# sweep takes one value with the settle disabled, because this gate is about the shape of
# the answer and a ten-frame settle per sample would buy nothing.
verbs=(
    "list|list"
    "info|info <camera>"
    "controls|controls <camera>"
    "get|get <camera> <control>"
    "set|set <camera> <control>=<value>"
    "snapshot|snapshot <camera>"
    "restore|restore <camera> <snapshot>"
    "photo|photo <camera> -o <photo>"
    "record|record <camera> -o <recording> --duration 100ms"
    "calibrate-start|calibrate start <camera> --task gate --goal gate-run"
    "calibrate-plan|calibrate plan <camera> --task gate <control>"
    "calibrate-sweep|calibrate sweep <camera> --task gate <control> --values <value> --skip-frames 0"
    "calibrate-status|calibrate status <camera> --task gate"
    "calibrate-select|calibrate select <camera> --task gate <control> --metric sharpness"
    "calibrate-apply|calibrate apply <camera> --task gate"
    "calibrate-restore|calibrate restore <camera> --task gate"
    "calibrate-list|calibrate list <camera>"
    "profile-capture|profile capture <camera>"
)

# The one home of "which verb answers with which document", found by name rather than by
# path: a table this gate had to be edited to follow would be a table that can move out from
# under it silently. `gate_find` prunes `target`, `.git`, `vendor` and `node_modules`, so the
# search is over this repository's own sources.
mapfile -t contract_tables < <(gate_find "$root" -name 'json-contracts.tsv' | tr '\0' '\n' | sort)
if ((${#contract_tables[@]} != 1)); then
    gate_fail "found ${#contract_tables[@]} json-contracts.tsv files under the tree; the verb-to-document mapping has exactly one home (design §2.10) and this gate reads it rather than repeating it"
    gate_finish
fi
contracts="${contract_tables[0]}"
gate_note "reading the --json contracts from ${contracts#"$root"/}"

# The document a verb answers with, or the empty string for a verb the table does not name.
json_contract() {
    awk -F'\t' -v verb="$1" '
        /^#/ { next }
        NF < 2 { next }
        $1 == verb { print $2; exit }
    ' "$contracts"
}

# Sorted, so the gate examines the same profile on every run and in every scratch copy —
# `find` has no defined order, and a gate whose subject varies run to run is a gate whose
# green means something different each time.
#
# The *first sorted profile with a writable integer control*, rather than simply the first:
# `get` and `set` need a control to name, and the corpus's first entry alphabetically
# (`chicony-ir`) exposes three controls, none of them writable. Still deterministic, and
# still derived from the tree rather than transcribed.
#
# **Two sentences and not one** (note **N248**). "There are no profiles" and "there are profiles
# and none of them can be written to" are one condition to this loop and two facts about the
# tree, with two different things to do about them: the first is the corpus floor gone, which is
# `corpus-floor.sh`'s subject, and the second is a re-capture that landed a device with nothing
# writable on it. They were one branch until 2026-08-17, and the arm named for the first of them
# was reported `ok` on the sentence belonging to the second — which is the L25 finding in its own
# case file.
mapfile -t candidates < <(gate_find "$root/corpus/profiles" -name '*.json' | tr '\0' '\n' | sort)
if ((${#candidates[@]} == 0)); then
    gate_fail "there are no committed device profiles under corpus/profiles/; this gate replays a device rather than asking for one to be attached, and there is nothing left to replay"
    gate_finish
fi
profile=""
for candidate in "${candidates[@]}"; do
    if [[ -n "$(writable_control "$candidate")" ]]; then
        profile="$candidate"
        break
    fi
done
if [[ -z "$profile" ]]; then
    gate_fail "none of the ${#candidates[@]} committed profile(s) exposes a writable integer control, so the write verbs cannot be exercised against a replayed device"
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

# Scratch space for the documents and bytes these rows produce. Under `target/` since the
# 2026-08-12 ruling, which is outside every tree walk — `gate_find` prunes `target` — so
# `no-frame-bytes-in-repo.sh` stays true of a directory full of sample photos, and a failed
# run leaves nothing behind that the next `gate_scratch_sweep` will not take.
scratch="$(mktemp -d "$(gate_scratch_root)/wch-json-validates.XXXXXXXX")"
trap 'rm -rf "$scratch"' EXIT

# The calibration verbs write a session tree (D9), and it goes in the scratch directory
# with everything else: sample photos are frames, the repository holds no frames
# (`no-frame-bytes-in-repo.sh`), and a gate that wrote into the operator's real state
# directory would put a session called "gate" in front of them every time CI ran.
export XDG_STATE_HOME="$scratch/state"

# The real checkout, whatever tree is under test — see the note above.
checkout="$(git rev-parse --show-toplevel)"
binary="$checkout/target/debug/webcam-handler-cli"
if [[ ! -x "$binary" ]]; then
    (cd "$checkout" && cargo build --locked --offline -p webcam-handler-cli --bin webcam-handler-cli >/dev/null 2>&1) ||
        {
            gate_fail "could not build webcam-handler-cli"
            gate_finish
        }
fi

checked=0
for row in "${verbs[@]}"; do
    IFS='|' read -r name argv <<<"$row"

    def="$(json_contract "$name")"
    if [[ -z "$def" ]]; then
        gate_fail "${contracts#"$root"/} names no document for '$name'; a verb with no \`--json\` contract is one the agent guide cannot teach and this gate cannot validate"
        continue
    fi

    argv="${argv//<camera>/cam:$camera_id}"
    argv="${argv//<control>/$control}"
    argv="${argv//<value>/$value}"
    argv="${argv//<photo>/$scratch/shot.jpg}"
    argv="${argv//<recording>/$scratch/take.avi}"
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
        gate_fail "webcam-handler-cli --json $argv exited $status; a verb that cannot answer cannot be validated"
        continue
    fi

    if ! printf '%s' "$output" | jq -e . >/dev/null 2>&1; then
        gate_fail "webcam-handler-cli --json $argv did not emit parseable JSON"
        continue
    fi

    reason="$(why_it_does_not_validate "$output" "$def")"
    if [[ -n "$reason" ]]; then
        gate_fail "webcam-handler-cli --json $argv does not match #/\$defs/$def in the committed bundle: $reason"
        continue
    fi

    # An answer must not wear the failure marker. A caller following the generated agent guide
    # branches on it before parsing anything else, so a verb whose answer carried it would be
    # read as a refusal on every successful run — which is the one direction of this ruling
    # nothing else here could notice.
    if printf '%s' "$output" | jq -e --arg m "$marker" 'type == "object" and has($m)' >/dev/null 2>&1; then
        gate_fail "webcam-handler-cli --json $argv answered with a document carrying '$marker', which is what tells an unattended caller that a verb refused"
        continue
    fi
    # Counted here, at the end, and not on entry: a row that failed to answer or failed
    # to validate was *attempted*, not validated, and `gate_checked` is the number the
    # report stands on.
    checked=$((checked + 1))
    gate_note "$name → #/\$defs/$def"
done

gate_checked "$checked" "--json verb answers validated against the committed bundle, each required not to carry the '$marker' marker"
gate_require_nonzero "$checked" "--json verb answers"

# ------------------------------------------------------------------ the refusals
#
# Two failures, chosen because they differ in the two ways that matter: one carries nothing
# beyond what the caller asked for and one carries the payload an agent acts on. A gate that
# drove only the first would pass a build whose document dropped every field it did not
# understand, which is the defect the owner's ruling is *about* — `available` is the retry.
refusals=(
    "camera-unknown|info cam:nothing-answers-to-this"
    "format-unsupported|photo <camera> -o <photo> --pixel-format NV12"
)

refused=0
declare -A refusal_codes=()
for row in "${refusals[@]}"; do
    IFS='|' read -r name argv <<<"$row"
    argv="${argv//<camera>/cam:$camera_id}"
    argv="${argv//<photo>/$scratch/refused.jpg}"

    status=0
    # shellcheck disable=SC2086
    output="$("$binary" --backend fake --profile "$profile" --json $argv 2>/dev/null)" || status=$?
    if ((status == 0)); then
        gate_fail "webcam-handler-cli --json $argv answered instead of refusing; this row exists to produce a failure and a fixture that stopped producing one would validate a document nobody met"
        continue
    fi
    # Not clap's, which would mean the command line was rejected before the camera was ever
    # asked — a usage error validated as a device refusal is this gate checking the wrong thing.
    if ((status == 2)); then
        gate_fail "webcam-handler-cli --json $argv was refused by clap (exit 2), so no D13 failure was produced to validate"
        continue
    fi

    if ! printf '%s' "$output" | jq -e . >/dev/null 2>&1; then
        gate_fail "webcam-handler-cli --json $argv printed no parseable document on standard output; a caller that redirected stdout would have lost the failure entirely (note N124)"
        continue
    fi
    reason="$(why_it_does_not_validate "$output" Failure)"
    if [[ -n "$reason" ]]; then
        gate_fail "webcam-handler-cli --json $argv does not match #/\$defs/Failure in the committed bundle: $reason"
        continue
    fi
    if ! printf '%s' "$output" | jq -e --arg m "$marker" '.[$m] == true' >/dev/null 2>&1; then
        gate_fail "webcam-handler-cli --json $argv printed a document that does not mark itself '$marker'; a caller cannot tell it from an answer"
        continue
    fi

    kind="$(printf '%s' "$output" | jq -r '.error.kind // empty')"
    if [[ -z "$kind" ]]; then
        gate_fail "webcam-handler-cli --json $argv printed a failure document with no discriminant; the whole of D13 is that busy and device_gone are told apart"
        continue
    fi
    known=0
    for spelling in "${d13_kinds[@]}"; do
        if [[ "$kind" == "$spelling" ]]; then known=1; fi
    done
    if ((known == 0)); then
        gate_fail "webcam-handler-cli --json $argv refused with kind '$kind', which is not one the D13 registry has"
        continue
    fi

    if [[ -n "${refusal_codes[$kind]:-}" && "${refusal_codes[$kind]}" != "$status" ]]; then
        gate_fail "'$kind' left exit $status here and ${refusal_codes[$kind]} elsewhere in this run"
        continue
    fi
    refusal_codes["$kind"]="$status"
    refused=$((refused + 1))
    gate_note "$name → #/\$defs/Failure, kind '$kind', exit $status"
done

# The payload, on the row that exists for it. `available` is what an agent retries with, and a
# document that named the refusal and dropped the list would leave it parsing the English
# sentence beside it — which is the state note **N124** measured.
formats="$("$binary" --backend fake --profile "$profile" --json \
    photo "cam:$camera_id" -o "$scratch/refused.jpg" --pixel-format NV12 2>/dev/null |
    jq -r '[.error.available[]? | select(type == "string")] | length' 2>/dev/null || true)"
if [[ -z "$formats" ]] || ((formats == 0)); then
    gate_fail "the format refusal carries no readable 'available' formats; that list is the retry an unattended caller makes, and a refusal without it is the English sentence wearing braces"
fi
gate_checked "$formats" "format(s) the format_unsupported refusal names as readable FourCCs for the caller to retry with"
gate_require_nonzero "$formats" "formats in the refusal payload"

# The redundant channel: distinct codes for distinct kinds. Compared against each other rather
# than against numbers written here — `cli_core::exit_code` is the one home of the mapping, and
# a gate that transcribed it would be a second table.
if ((${#refusal_codes[@]} > 0)); then
    # Guarded, because `printf '%s\n'` with no arguments prints one empty line and would
    # report a phantom collision on a run where every row above had already failed.
    distinct="$(printf '%s\n' "${refusal_codes[@]}" | sort -u | wc -l | tr -d ' ')"
    if ((distinct != ${#refusal_codes[@]})); then
        gate_fail "${#refusal_codes[@]} refusal kind(s) left only $distinct distinct exit code(s); the codes are the document's redundant half and two kinds sharing one is the collapse D13 exists to prevent"
    fi
fi
gate_checked "$refused" "--json refusal(s) validated against #/\$defs/Failure, each marked '$marker', naming a D13 kind, and exiting a code of its own"
gate_require_nonzero "$refused" "--json refusals"

# Every verb the CLI offers must have a row above. Derived from `--help`, so a verb added
# without a row is a failure rather than a quiet omission — and derived at **both levels**,
# because two of the ten top-level names are subtrees.
#
# The single-level version of this loop was the P3 review's finding: it scraped only
# `webcam-handler-cli --help`, and its membership test accepted a top-level verb if *any* row's
# name began with it, so one `calibrate-start` row satisfied the whole seven-verb `calibrate`
# subtree. Deleting the other six rows left the gate green while it validated six fewer
# documents — the criterion in `phase-criteria.tsv`, docs/7 §P3d and docs/9 all assert a
# property the predicate did not have. Before P3 the gap was latent (`profile` had one
# subcommand); P3d made `calibrate` a seven-verb tree and made it load-bearing. Note N10's
# family, again: a gate green while checking less than it claims.
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
