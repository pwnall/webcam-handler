# Both-direction cases for `no-frame-bytes-in-repo.sh`.
#
# The fixtures are built byte by byte rather than committed, because committing an image
# to prove that images are rejected would be the joke that writes itself. The JPEGs are
# marker chains with a real SOF0 frame header — enough for the predicate's parser, and
# not a picture of anything.
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
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_fixture_without_provenance() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/corpus/images"
    _write_jpeg "$tree/corpus/images/frame.jpg" 64 64
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Over the cap: 640x480 is the smallest mode a webcam commonly offers.
fail_case_fixture_over_the_dimension_cap() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/corpus/images"
    _write_jpeg "$tree/corpus/images/vga.jpg" 640 480
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/corpus/images/vga.jpg.provenance.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_fixture_in_an_unexpected_format() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/corpus/images"
    printf 'GIF89a\x01\x00\x01\x00\x00\x00\x00generated-by: the gate selftest' \
        >"$tree/corpus/images/pattern.gif"
    WCH_GATE_ROOT="$tree" "$GATE"
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
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_avi_without_provenance() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/avi"
    _write_avi "$tree/crates/imaging/fixtures/avi/unmarked.avi" 64 48
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The same cap the still fixtures get, read out of `avih` rather than out of a SOF0.
fail_case_avi_over_the_dimension_cap() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/imaging/fixtures/avi"
    _write_avi "$tree/crates/imaging/fixtures/avi/vga.avi" 640 480
    printf 'generated-by = "the gate selftest"\n' \
        >"$tree/crates/imaging/fixtures/avi/vga.avi.provenance.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
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
    WCH_GATE_ROOT="$tree" "$GATE"
}

# An empty tree sniffs nothing, and a check that examined nothing cannot go red.
fail_case_nothing_to_scan() {
    local tree
    tree="$(mktemp -d "$(gate_scratch_root)/wch-empty-tree.XXXXXXXX")"
    WCH_GATE_ROOT="$tree" "$GATE"
}
