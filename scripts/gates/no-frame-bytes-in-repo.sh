#!/usr/bin/env bash
#
# A frame may contain a person (design §5, AGENTS.md "Hardware and privacy"). Camera
# frames never enter the repository. This is a content check, not a convention: file
# extensions are advisory and `.gitignore` is not a privacy control, so every file in the
# tree is sniffed for an image magic number.
#
# Each format has exactly one directory it may live in, and it may live there only under
# three conditions (design §3.2: synthetic fixtures only):
#
#   1. It carries a `generated-by` provenance marker — in the bytes (a PNG `tEXt` chunk
#      or a JPEG comment segment both satisfy this) or in a `<file>.provenance.toml`
#      sidecar. A fixture that cannot say what produced it is indistinguishable from a
#      frame that came off a camera.
#   2. It is a PNG, a JPEG or an AVI — the three formats this tool emits. Anything else in
#      a fixture directory arrived from somewhere unexamined.
#   3. It is small. The cap below is deliberately under VGA: 640x480 is the smallest mode
#      a webcam commonly offers, so a fixture that exceeds the cap is either a frame or a
#      fixture that should have been generated smaller.
#
# ## Why a container is sniffed at all, which P6a is the answer to
#
# The still-image half of this gate is old. The video half is not, and it landed because
# the P6a review found the hole it fills: `crates/imaging/fixtures/avi/two-frame-64x48.avi`
# put **two JPEG bitstreams into the tree** where nothing could see them. A `RIFF`/`AVI `
# container matches no image magic number — the WEBP arm below needs `WEBP` at offset 8 —
# so the file reported as "not an image", the run stayed green, and the only thing standing
# behind it was one in-crate assertion covering one file by name. A second AVI committed
# anywhere tomorrow would have been invisible to every predicate in this suite, and rule 1
# asks for a gate that can go red on the *class* rather than prose about it.
#
# So an AVI is walked to its `avih` and held to the same three conditions, with the frame
# extent read out of the header rather than out of a still image's. Provenance for a
# container is a sidecar: our muxer writes no comment chunk, and adding one to carry a
# marker would move the frozen bytes the fixture exists to freeze.
#
# Honest limit, recorded in docs/9 and design §3.3 item 6: this sniffs *known* formats. A
# frame inside a container this file does not walk still passes, and review carries that
# half — the list is now four still formats and one container rather than four and none,
# which narrows the gap without closing it.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# Under VGA on both axes — see condition 3 above.
max_fixture_dim=512
# One mebibyte. A synthetic pattern that needs more is a camera frame by another name,
# and the cap also bounds the cost of the dimension parse below.
max_fixture_bytes=1048576

# One home per format, because they are different fixture families with different owners.
# `corpus/images/` is design §3.2's synthetic still-image directory; the AVI belongs to one
# crate's muxer and lives with it, the same split `crates/backends/v4l2/fixtures/` makes.
image_dir="corpus/images"
video_dir="crates/imaging/fixtures/avi"

# Where a file of this format is allowed to be.
home_for() {
    case "$1" in
    avi) printf '%s\n' "$video_dir" ;;
    *) printf '%s\n' "$image_dir" ;;
    esac
}

# ------------------------------------------------------------------ sniffing

# Echo the format name for a file, or nothing.
sniff() {
    local file="$1" head6 head4 head12 size declared
    head12="$(od -An -v -tx1 -N12 "$file" | tr -d ' \n')"
    head6="${head12:0:12}"
    head4="${head12:0:8}"

    case "$head12" in
    89504e470d0a1a0a*) printf 'png\n' ;;
    ffd8ff*) printf 'jpeg\n' ;;
    esac
    case "$head4" in
    47494638) printf 'gif\n' ;;
    esac
    if [[ "$head4" == "52494646" && "${head12:16:8}" == "57454250" ]]; then
        printf 'webp\n'
    fi
    # The same envelope with our own form type. `52494646` is `RIFF` and `41564920` is
    # `AVI ` — the trailing space is part of the FourCC, and a form type this specific
    # cannot be two plausible bytes of anything else.
    if [[ "$head4" == "52494646" && "${head12:16:8}" == "41564920" ]]; then
        printf 'avi\n'
    fi
    # "BM" alone is two plausible bytes of text, so a BMP must also agree with itself
    # about its own size before we call it one.
    if [[ "${head6:0:4}" == "424d" ]]; then
        size="$(wc -c <"$file")"
        declared="$(od -An -j2 -N4 -tu4 --endian=little "$file" | tr -d ' ')"
        if [[ "$declared" == "$size" ]]; then
            printf 'bmp\n'
        fi
    fi
}

png_dimensions() {
    od -An -j16 -N8 -tu4 --endian=big -v "$1" | tr -s ' ' ' ' | sed 's/^ //;s/ $//'
}

# Walk the JPEG marker chain to the first SOFn and read its frame header. Written out
# rather than shelled to `identify`, because an external image tool is not a build or CI
# dependency of this project (AGENTS.md: no runtime external binaries).
jpeg_dimensions() {
    od -An -v -tu1 -w1 "$1" | awk '
        { b[NR] = $1 + 0 }
        END {
            i = 3
            while (i <= NR) {
                if (b[i] != 255) { i++; continue }
                while (i <= NR && b[i] == 255) i++
                if (i > NR) break
                m = b[i]; i++
                if (m == 216 || m == 1 || (m >= 208 && m <= 215)) continue
                if (m == 217) break
                if (i + 1 > NR) break
                len = b[i] * 256 + b[i + 1]
                if (m >= 192 && m <= 207 && m != 196 && m != 200 && m != 204) {
                    printf "%d %d\n", b[i + 5] * 256 + b[i + 6], b[i + 3] * 256 + b[i + 4]
                    exit
                }
                if (m == 218) break
                i += len
            }
        }'
}

# The frame extent an AVI declares, out of `avih` — nothing, if this is not an AVI whose
# header can be walked to.
#
# A walk of two chunks rather than a search for a byte pattern: the specification puts
# `LIST hdrl` first inside the `AVI ` form and `avih` first inside `hdrl`, so the three
# FourCCs below sit at fixed offsets in any conforming file, and `avih.dwWidth`/`dwHeight`
# are at +32 and +36 of its payload. A file that does not match prints nothing, which the
# caller turns into a failure — an AVI whose geometry cannot be read is an AVI nobody
# checked, and this gate does not have a second way to find out how big its frames are.
avi_dimensions() {
    local file="$1" width height
    # 12 the first chunk's id, 20 its list type, 24 the first chunk inside it: `LIST`,
    # `hdrl`, `avih` as hex.
    [[ "$(od -An -j12 -N4 -tx1 -v "$file" | tr -d ' \n')" == "4c495354" ]] || return 0
    [[ "$(od -An -j20 -N4 -tx1 -v "$file" | tr -d ' \n')" == "6864726c" ]] || return 0
    [[ "$(od -An -j24 -N4 -tx1 -v "$file" | tr -d ' \n')" == "61766968" ]] || return 0
    width="$(od -An -j64 -N4 -tu4 --endian=little -v "$file" | tr -d ' ')"
    height="$(od -An -j68 -N4 -tu4 --endian=little -v "$file" | tr -d ' ')"
    [[ -n "$width" && -n "$height" ]] || return 0
    printf '%s %s\n' "$width" "$height"
}

has_provenance() {
    local file="$1" sidecar
    if LC_ALL=C grep -qaiE 'generated[-_]by' "$file"; then
        return 0
    fi
    for sidecar in "$file.provenance.toml" "$file.provenance.json"; do
        if [[ -f "$sidecar" ]] && LC_ALL=C grep -qaiE 'generated[-_]by' "$sidecar"; then
            return 0
        fi
    done
    return 1
}

# ------------------------------------------------------------------ the walk

sniffed=0
images=0
fixtures=0
while IFS= read -r -d '' file; do
    sniffed=$((sniffed + 1))
    format="$(sniff "$file")"
    if [[ -z "$format" ]]; then
        continue
    fi
    images=$((images + 1))
    rel="${file#"$root"/}"
    home="$(home_for "$format")"

    if [[ "$rel" != "$home/"* ]]; then
        gate_fail "$rel is a committed $format carrying frame data; a $format lives only in $home/ (design §5: a frame may contain a person)"
        continue
    fi

    fixtures=$((fixtures + 1))

    if ! has_provenance "$file"; then
        gate_fail "$rel carries no generated-by provenance marker; an unprovenanced fixture is indistinguishable from a camera frame"
        continue
    fi

    size="$(wc -c <"$file")"
    if ((size > max_fixture_bytes)); then
        gate_fail "$rel is $size bytes, over the $max_fixture_bytes-byte fixture cap"
        continue
    fi

    case "$format" in
    png) dims="$(png_dimensions "$file")" ;;
    jpeg) dims="$(jpeg_dimensions "$file")" ;;
    avi) dims="$(avi_dimensions "$file")" ;;
    *)
        gate_fail "$rel is a $format; the fixture directories hold PNG, JPEG and AVI fixtures only"
        continue
        ;;
    esac

    read -r width height <<<"$dims"
    if [[ -z "${width:-}" || -z "${height:-}" ]]; then
        gate_fail "$rel is a $format whose frame extent could not be read; an unreadable fixture is not a checked fixture"
        continue
    fi
    if ((width > max_fixture_dim || height > max_fixture_dim)); then
        gate_fail "$rel is ${width}x${height}, over the ${max_fixture_dim}px fixture cap (under VGA on purpose)"
    fi
done < <(gate_find "$root")

gate_checked "$sniffed" "files content-sniffed for image magic numbers"
gate_require_nonzero "$sniffed" "files"
gate_checked "$images" "files carrying frame data found in the tree"

if ((fixtures == 0)); then
    gate_skip 0 "provenanced fixtures validated — neither $image_dir/ nor $video_dir/ holds one; the nothing-anywhere-else claim above is what carries this run"
else
    gate_checked "$fixtures" "fixtures in $image_dir/ and $video_dir/ checked for provenance, format, size and frame extent"
fi

gate_finish
