# Both-direction cases for `no-frame-bytes-in-repo.sh`.
#
# The fixtures are built byte by byte rather than committed, because committing an image
# to prove that images are rejected would be the joke that writes itself. The JPEGs are
# marker chains with a real SOF0 frame header — enough for the predicate's parser, and
# not a picture of anything.
#
# Each arm names the sentence it is claiming (`gate_red_because`, note **N31**), and here that is
# almost the whole content of an arm: thirteen of the fourteen seeds differ from one another only
# in *which* file, in *which* directory, in *which* format, tripped *which* of five sentences —
# and the exit status is the same number for all of them. So each arm claims the file it wrote
# and the sentence that file must produce, which is what stops the format arms from silently
# collapsing onto the "lives only in" branch the day a home moves.
#
# shellcheck shell=bash

# A 1x1 PNG.
_write_png() {
    printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGMAAQAABQABDQottAAAAABJRU5ErkJggg==' |
        base64 -d >"$1"
}

_be16() {
    printf '%b' "$(printf '\\x%02x\\x%02x' "$(($1 / 256))" "$(($1 % 256))")"
}

# A JPEG whose SOF0 declares $2 x $3. Marker chain only: there is no image in it.
_write_jpeg() {
    local out="$1" width="$2" height="$3"
    {
        printf '\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00'
        printf '\xff\xc0\x00\x11\x08'
        _be16 "$height"
        _be16 "$width"
        printf '\x03\x01\x22\x00\x02\x11\x01\x03\x11\x01\xff\xd9'
    } >"$out"
}

_le32() {
    local v="$1"
    printf '%b' "$(printf '\\x%02x\\x%02x\\x%02x\\x%02x' \
        "$((v & 255))" "$(((v >> 8) & 255))" "$(((v >> 16) & 255))" "$(((v >> 24) & 255))")"
}

_zeros() {
    local n="$1" i
    for ((i = 0; i < n; i++)); do printf '\x00'; done
}

# An AVI whose `avih` declares $2 x $3. Header list only: there is no frame in it, which is
# the point — the predicate reads the declared extent, not pixels.
_write_avi() {
    local out="$1" width="$2" height="$3"
    {
        printf 'RIFF'
        _le32 92
        printf 'AVI '
        printf 'LIST'
        _le32 68
        printf 'hdrl'
        printf 'avih'
        _le32 56
        _zeros 32
        _le32 "$width"
        _le32 "$height"
        _zeros 16
        printf 'LIST'
        _le32 4
        printf 'movi'
    } >"$out"
}

pass_case() {
    "$GATE"
}

# Provenance embedded in the bytes — what a PNG `tEXt` chunk or a JPEG comment segment
# looks like to a content check.
pass_case_provenanced_fixture_with_embedded_marker() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/corpus/images"
    _write_png "$tree/corpus/images/checkerboard.png"
    printf 'generated-by: webcam-handler xtask fixtures\n' >>"$tree/corpus/images/checkerboard.png"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Provenance in a sidecar, for a format with nowhere convenient to put it.
pass_case_provenanced_fixture_with_sidecar() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/corpus/images"
    _write_jpeg "$tree/corpus/images/gradient.jpg" 64 64
    printf 'generated-by = "webcam-handler xtask fixtures"\n' \
        >"$tree/corpus/images/gradient.jpg.provenance.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_image_outside_the_fixture_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    _write_png "$tree/docs/screenshot.png"
    gate_red_because 'docs/screenshot.png is a committed png carrying frame data' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_fixture_without_provenance() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/corpus/images"
    _write_jpeg "$tree/corpus/images/frame.jpg" 64 64
    gate_red_because 'corpus/images/frame.jpg carries no generated-by provenance marker' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# Over the cap: 640x480 is the smallest mode a webcam commonly offers.
fail_case_fixture_over_the_dimension_cap() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/corpus/images"
    _write_jpeg "$tree/corpus/images/vga.jpg" 640 480
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/corpus/images/vga.jpg.provenance.toml"
    gate_red_because 'corpus/images/vga.jpg is 640x480, over the' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_fixture_in_an_unexpected_format() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/corpus/images"
    printf 'GIF89a\x01\x00\x01\x00\x00\x00\x00generated-by: the gate selftest' \
        >"$tree/corpus/images/pattern.gif"
    gate_red_because 'corpus/images/pattern.gif is a gif;' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The shape the P6a review found: a container carrying frames, in the one directory that
# owns it, with the marker in a sidecar because the muxer writes no comment chunk.
pass_case_provenanced_avi_fixture() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/avi"
    _write_avi "$tree/crates/imaging/fixtures/avi/generated.avi" 64 48
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/crates/imaging/fixtures/avi/generated.avi.provenance.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The finding itself: before P6a's repair, an AVI anywhere in the tree matched no magic
# number and the run stayed green.
fail_case_avi_outside_its_fixture_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    _write_avi "$tree/docs/recording.avi" 64 48
    gate_red_because 'docs/recording.avi is a committed avi carrying frame data' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_avi_without_provenance() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/avi"
    _write_avi "$tree/crates/imaging/fixtures/avi/unmarked.avi" 64 48
    gate_red_because 'avi/unmarked.avi carries no generated-by provenance marker' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The same cap the still fixtures get, read out of `avih` rather than out of a SOF0.
fail_case_avi_over_the_dimension_cap() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/avi"
    _write_avi "$tree/crates/imaging/fixtures/avi/vga.avi" 640 480
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/crates/imaging/fixtures/avi/vga.avi.provenance.toml"
    gate_red_because 'avi/vga.avi is 640x480, over the' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# A provenanced AVI whose header this predicate cannot walk to. An extent it cannot read is
# an extent it cannot bound, and a fixture nobody checked must not read as a checked one.
fail_case_avi_whose_frame_extent_cannot_be_read() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/avi"
    {
        printf 'RIFF'
        _le32 92
        printf 'AVI '
        printf 'JUNK'
        _le32 80
        _zeros 80
    } >"$tree/crates/imaging/fixtures/avi/headerless.avi"
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/crates/imaging/fixtures/avi/headerless.avi.provenance.toml"
    gate_red_because 'avi/headerless.avi is a avi whose frame extent could not be read' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# A Y4M whose header declares $2 x $3. Header line only: there is no frame in it, which is
# the point — the predicate reads the declared extent, not samples. `Ip` and `C420` are in it
# because the fields between `H` and `C` are what a naive "the third field is the
# colorspace" reader would trip over, and this predicate must not be one.
_write_y4m() {
    local out="$1" width="$2" height="$3"
    printf 'YUV4MPEG2 W%s H%s F1000000:33333 Ip C420\n' "$width" "$height" >"$out"
}

# The shape P6b landed: raw picture, in the one directory that owns it, with the marker in a
# sidecar because the header has nowhere to put one that would not move the frozen bytes.
pass_case_provenanced_y4m_fixture() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/y4m"
    _write_y4m "$tree/crates/imaging/fixtures/y4m/generated.y4m" 64 48
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/crates/imaging/fixtures/y4m/generated.y4m.provenance.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The finding itself, repeated one container along: before this arm, a Y4M anywhere in the
# tree matched no magic number and the run stayed green — over a format whose payload is one
# luma sample per byte.
fail_case_y4m_outside_its_fixture_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    _write_y4m "$tree/docs/capture.y4m" 64 48
    gate_red_because 'docs/capture.y4m is a committed y4m carrying frame data' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_y4m_without_provenance() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/y4m"
    _write_y4m "$tree/crates/imaging/fixtures/y4m/unmarked.y4m" 64 48
    gate_red_because 'y4m/unmarked.y4m carries no generated-by provenance marker' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# An AVI in the Y4M directory and a Y4M in the AVI directory are both a fixture that got
# loose from the module that owns it, and each format has exactly one home.
fail_case_y4m_in_the_avi_fixture_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/avi"
    _write_y4m "$tree/crates/imaging/fixtures/avi/stray.y4m" 64 48
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/crates/imaging/fixtures/avi/stray.y4m.provenance.toml"
    gate_red_because 'avi/stray.y4m is a committed y4m carrying frame data' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The same cap the still fixtures get, read out of the header line rather than out of a SOF0.
fail_case_y4m_over_the_dimension_cap() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/y4m"
    _write_y4m "$tree/crates/imaging/fixtures/y4m/vga.y4m" 640 480
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/crates/imaging/fixtures/y4m/vga.y4m.provenance.toml"
    gate_red_because 'y4m/vga.y4m is 640x480, over the' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# A provenanced Y4M whose header names no height. An extent it cannot read is an extent it
# cannot bound, and a fixture nobody checked must not read as a checked one.
fail_case_y4m_whose_frame_extent_cannot_be_read() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/y4m"
    printf 'YUV4MPEG2 W64 F1000000:33333 C420\n' \
        >"$tree/crates/imaging/fixtures/y4m/headerless.y4m"
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/crates/imaging/fixtures/y4m/headerless.y4m.provenance.toml"
    gate_red_because 'y4m/headerless.y4m is a y4m whose frame extent could not be read' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# An empty tree sniffs nothing, and a check that examined nothing cannot go red.
fail_case_nothing_to_scan() {
    local tree
    tree="$(mktemp -d "$(gate_scratch_root)/wch-empty-tree.XXXXXXXX")"
    gate_red_because 'examined zero files' env WCH_GATE_ROOT="$tree" "$GATE"
}
