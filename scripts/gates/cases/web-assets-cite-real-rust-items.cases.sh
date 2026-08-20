# Both-direction cases for `web-assets-cite-real-rust-items.sh`.
#
# The predicate resolves the Rust paths the shipped client's prose cites, and the arm it exists
# for is `fail_case_a_module_that_is_not_there`: that is the shape the defect actually took when
# D20's `/session-photo` landed, with `credential.js` naming `daemon::http::samples` — a module
# no build of this workspace has ever had — beside a route whose spelling happened to be right.
#
# The green arms are the shapes it must *allow*, and they are the ones that make the walk more
# than a word search in the other direction: an item that is not a module (`while_suspended` is a
# function, and the segments before it are modules), a field reached through a type, and the
# crate-less shorthands this client legitimately writes.
#
# The second rule's arms are `fail_case_a_repository_path_that_is_not_there` and its sibling in
# `crates/web/src`, and they exist because the class shipped its second spelling in the same
# commit as the ban on its first: two files told a reader that
# `scripts/gates/web-client-urls-sync.sh` reconciles the page's wire names, and this predicate was
# green over both (note **N284**).
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

pass_case_a_function_is_not_a_module() {
    # `engine::preview::while_suspended` is two modules and a `pub fn`, and a walk that insisted
    # every lowercase segment be a module would refuse the most load-bearing citation in this
    # client. Seeded rather than relied on, so the arm still means something the day photo.js
    # stops citing it.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/web/assets/dom.js" <<'JS'

// A function, cited: `engine::preview::while_suspended` is where the pause lives.
JS
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_field_reached_through_its_type() {
    # `schema::session::Session::criteria` is a module, a struct and one of its fields. The
    # predicate's header prices the coarseness that allows this; the arm is what stops somebody
    # tightening it into a rule that refuses every citation of a field.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/web/assets/dom.js" <<'JS'

// A field, cited: `schema::session::Session::criteria` is what the selector judges against.
JS
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_crate_less_shorthand_is_counted_rather_than_resolved() {
    # `limits::…` and `render::…` name homes whose crate the surrounding sentence makes obvious,
    # and resolving one would mean this predicate guessing. They are counted and named in the
    # note instead — which is a decision, so it has an arm.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/web/assets/dom.js" <<'JS'

// A shorthand: `limits::A_BOUND_THAT_IS_NOT_THERE` and `render::a_function_that_is_not_there`.
JS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_module_that_is_not_there() {
    # **The instance that shipped.** `credential.js` cited `daemon::http::samples`; the module is
    # `session_photo`, and `samples` appears in `daemon::http`'s own prose often enough that a
    # word search would have passed it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/daemon::http::session_photo::SESSION_PHOTO_PATH/daemon::http::samples::SESSION_PHOTO_PATH/' \
        "$tree/crates/web/assets/credential.js"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and all, matched verbatim
    gate_red_because 'declares no `samples` — nor is it a module of it' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_constant_the_named_module_does_not_declare() {
    # The other half of the same citation: the module is right and the item is not, which is what
    # a rename inside `daemon::http::session_photo` leaves behind.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/daemon::http::session_photo::PASS_QUERY_PARAM/daemon::http::session_photo::SWEEP_PASS_QUERY_PARAM/' \
        "$tree/crates/web/assets/credential.js"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and all, matched verbatim
    gate_red_because 'declares no `SWEEP_PASS_QUERY_PARAM`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_crate_this_workspace_does_not_have() {
    # A citation whose first segment names a crate this predicate is told to know and the tree
    # does not have — the shape a crate directory moving leaves, and the reason the table is
    # checked against the filesystem rather than trusted.
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/crates/api/src"
    gate_red_because 'and there is no such directory' env WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_repository_path_that_is_there() {
    # The second rule's green direction, seeded so the arm still means something the day the
    # citations it reads today are reworded.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/web/assets/dom.js" <<'JS'

// A path, cited: `scripts/gates/no-external-fetch-in-web.sh` is what keeps this directory offline.
JS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_repository_path_that_is_not_there() {
    # **The instance that shipped beside the first rule.** `credential.js` told a reader that
    # `scripts/gates/web-client-urls-sync.sh` makes the same comparison over the source; no such
    # file has ever existed, and a predicate that read only `crate::path` citations could not see
    # it (note **N284**).
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|scripts/gates/web-assets-cite-real-rust-items.sh|scripts/gates/web-client-urls-sync.sh|' \
        "$tree/crates/web/assets/credential.js"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and all, matched verbatim
    gate_red_because 'cites `scripts/gates/web-client-urls-sync.sh`, and this tree has no such file' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_repository_path_the_serving_crate_names() {
    # The other half of the same instance, and the reason `crates/web/src` is in the walk: the
    # crate whose header documents this client said the same untrue thing about the same file.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|scripts/gates/web-assets-cite-real-rust-items.sh|scripts/gates/web-client-urls-sync.sh|' \
        "$tree/crates/web/src/lib.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and all, matched verbatim
    gate_red_because 'crates/web/src/lib.rs cites `scripts/gates/web-client-urls-sync.sh`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_bad_citation_in_the_page_rather_than_a_module() {
    # `index.html` is a shipped asset and it carries citations — D20's criteria field names
    # `schema::session::Session::criteria` in the comment beside it. The population read ten of
    # twelve files until 2026-08-20, so a citation written here was outside the check that exists
    # to hold it (note **N284**).
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/schema::session::Session::criteria/schema::sessions::Session::criteria/' \
        "$tree/crates/web/assets/index.html"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and all, matched verbatim
    gate_red_because 'declares no `sessions` — nor is it a module of it' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_client_that_cites_nothing_at_all() {
    # Zero is a failure, not a pass: a client whose prose stopped naming its Rust homes empties
    # this predicate's population, and a check that examines nothing cannot go red.
    local tree
    tree="$(gate_scratch_tree)"
    local asset
    for asset in "$tree"/crates/web/assets/*; do
        # shellcheck disable=SC2016  # the citation form this predicate reads, removed literally
        perl -0pi -e 's/`[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z0-9_]+)+`/`a citation nobody wrote`/g' "$asset"
    done
    gate_red_because 'examined zero crate-qualified Rust item paths' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_client_that_cites_no_repository_path_at_all() {
    # The same rule for the second population: prose that stopped naming the files it sends a
    # reader to is a check with nothing left to examine.
    local tree
    tree="$(gate_scratch_tree)"
    local cited
    for cited in "$tree"/crates/web/assets/* "$tree"/crates/web/src/*.rs; do
        # shellcheck disable=SC2016  # the citation form this predicate reads, removed literally
        perl -0pi -e 's/`[A-Za-z0-9_.\/-]+\.(sh|rs)`/`a path nobody wrote`/g' "$cited"
    done
    gate_red_because 'examined zero repository paths' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_client_with_no_modules_left() {
    # The other empty population, and the other direction of the same rule: nothing shipped under
    # `crates/web/assets` at all.
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree"/crates/web/assets/*
    gate_red_because 'examined zero shipped client modules' env WCH_GATE_ROOT="$tree" "$GATE"
}
