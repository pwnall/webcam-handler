# Both-direction cases for `browser-pins-sync.sh`.
#
# One failing arm per copy of the fact and per way a copy can go quiet: a README that names the
# wrong version, a manifest bumped with the README left behind, a README that stopped naming a
# pin at all, a pin that grew a range operator, a lockfile that drifted, a pin the README has no
# sentence for, a citation that points at nothing, and the fact itself gone. Each arm names the
# sentence it is claiming (`gate_red_because`), because a seeded pin bump is red under more than
# one branch of this predicate and an arm reading only the exit status cannot tell which one
# fired.
#
# Every seeded value is *new* — 9.9.9, 4321, 999.0.0.1 — while every value the seed has to
# *find* is read out of the scratch tree's own manifest. An arm that transcribed the shipped pin
# would stop seeding anything the day the pin moved, and would do it silently.
#
# shellcheck shell=bash

_manifest() {
    printf '%s' "$1/crates/daemon/tests/browser/package.json"
}

_pinned_playwright() {
    jq -r '.devDependencies["@playwright/test"]' "$(_manifest "$1")"
}

pass_case() {
    "$GATE"
}

pass_case_the_readme_may_wrap_between_a_label_and_its_value() {
    # A shape the predicate must allow. The sentence carrying these numbers is prose and it
    # wraps; where a paragraph happens to break is not something a version check may have an
    # opinion about, and a line-by-line implementation would go red here.
    local tree
    tree="$(gate_scratch_tree)"
    sed 's/Chromium build /Chromium build\n/' "$tree/README.md" >"$tree/README.md.seeded"
    mv "$tree/README.md.seeded" "$tree/README.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_readme_states_another_playwright_version() {
    local tree pinned
    tree="$(gate_scratch_tree)"
    pinned="$(_pinned_playwright "$tree")"
    sed "s/\`$pinned\`/\`9.9.9\`/" "$tree/README.md" >"$tree/README.md.seeded"
    mv "$tree/README.md.seeded" "$tree/README.md"
    gate_red_because 'README.md says @playwright/test is 9.9.9' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_readme_states_another_chromium_build() {
    local tree build
    tree="$(gate_scratch_tree)"
    build="$(jq -r '.wch.chromiumBuild' "$(_manifest "$tree")")"
    sed "s/\`$build\`/\`4321\`/" "$tree/README.md" >"$tree/README.md.seeded"
    mv "$tree/README.md.seeded" "$tree/README.md"
    gate_red_because 'README.md says Chromium build is 4321' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_manifest_was_bumped_and_the_readme_left_behind() {
    # The failure this predicate exists for, and the one nobody would notice: the browser moves,
    # `npm ci` and `pins.spec.mjs` both agree with the new number, every test stays green, and
    # the README goes on describing a browser this project no longer drives.
    local tree manifest
    tree="$(gate_scratch_tree)"
    manifest="$(_manifest "$tree")"
    jq '.wch.chromiumVersion = "999.0.0.1"' "$manifest" >"$manifest.seeded"
    mv "$manifest.seeded" "$manifest"
    gate_red_because 'pins 999.0.0.1' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_readme_stopped_naming_two_of_the_pins() {
    # The silent case. A claim that vanishes must not pass: a README that no longer says which
    # browser `just rung-web-install` fetches is not a README that agrees with the manifest.
    local tree pinned
    tree="$(gate_scratch_tree)"
    pinned="$(_pinned_playwright "$tree")"
    sed "/\`$pinned\`/d" "$tree/README.md" >"$tree/README.md.seeded"
    mv "$tree/README.md.seeded" "$tree/README.md"
    gate_red_because 'README.md names no version for @playwright/test' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_manifest_pin_grew_a_range_operator() {
    # What `npm install --save @playwright/test` writes. `pins.spec.mjs` catches it too — on a
    # host that has node, which upstream CI does not, so this arm is the one that runs everywhere.
    local tree manifest
    tree="$(gate_scratch_tree)"
    manifest="$(_manifest "$tree")"
    jq '.devDependencies["@playwright/test"] = "^" + .devDependencies["@playwright/test"]' \
        "$manifest" >"$manifest.seeded"
    mv "$manifest.seeded" "$manifest"
    gate_red_because 'is not an exact version' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_lockfile_states_a_version_the_manifest_does_not() {
    local tree lockfile
    tree="$(gate_scratch_tree)"
    lockfile="$tree/crates/daemon/tests/browser/package-lock.json"
    jq '.packages["node_modules/@playwright/test"].version = "9.9.9"' \
        "$lockfile" >"$lockfile.seeded"
    mv "$lockfile.seeded" "$lockfile"
    gate_red_because 'run npm ci under crates/daemon/tests/browser/' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_manifest_declares_a_pin_the_readme_has_no_sentence_for() {
    # The derived population's own arm: a fourth pin is a fourth thing the README's reader is
    # entitled to know, and a predicate that skipped what it did not recognise would turn "the
    # README states the pins" into "the README states the pins somebody remembered".
    local tree manifest
    tree="$(gate_scratch_tree)"
    manifest="$(_manifest "$tree")"
    jq '.wch.firefoxBuild = "77"' "$manifest" >"$manifest.seeded"
    mv "$manifest.seeded" "$manifest"
    gate_red_because "declares the pin 'firefoxBuild'" env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_readme_no_longer_cites_the_manifest() {
    local tree
    tree="$(gate_scratch_tree)"
    sed 's|crates/daemon/tests/browser/package.json|crates/daemon/tests/browser/pins.json|g' \
        "$tree/README.md" >"$tree/README.md.seeded"
    mv "$tree/README.md.seeded" "$tree/README.md"
    gate_red_because 'never names crates/daemon/tests/browser/package.json' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_fact_itself_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$(_manifest "$tree")"
    gate_red_because 'the browser pins have no home to reconcile against' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
