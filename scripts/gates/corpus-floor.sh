#!/usr/bin/env bash
#
# Dead corpus: a committed device profile nobody replays (design §3.2, docs/4 P1 row).
#
# A profile is expensive — it takes hardware somebody has, at a kernel version somebody
# ran — and it is committed precisely so the behaviour it records outlives the machine. A
# profile no test loads records nothing: it is a file that can rot for a year and turn
# nothing red. This predicate is the floor under that.
#
# Three claims, each derived rather than transcribed:
#
#   1. **The corpus is not empty.** From P1, `corpus/profiles/` holds the captures. An
#      empty directory would make every claim built on the corpus vacuous, which is the
#      exact shape of failure the whole suite exists to catch.
#   2. **Every profile is reachable from a test.** Either a test names it, or a test walks
#      the whole directory (`corpus::load_all` / `corpus::profile_paths`). The walk is the
#      better answer — it covers a profile added tomorrow without anybody remembering —
#      and this predicate accepts it *as* coverage rather than demanding both.
#   3. **At least one test replays the corpus through a backend.** Claims 1 and 2 would be
#      satisfied by a test that only parsed the documents, and "it is valid JSON" is not
#      what a device profile is for. The replaying test is found by looking for a file
#      that both reaches the corpus and constructs a backend from it.
#
# The corpus side of every population is a `find` over the directory; the test side is a
# grep over the tree's Rust sources. Neither is a list anybody maintains.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
profiles_dir="$root/corpus/profiles"

# The functions that reach the whole corpus at once, and the constructor that turns a
# document back into a device. Both are API names in this workspace, so a rename shows up
# here as a failure rather than as a silently weaker gate.
walker_pattern='corpus::(load_all|profile_paths)\('
replay_pattern='FakeBackend::(new|from_profile)\('

# ------------------------------------------------------------------ claim 1

if [[ ! -d "$profiles_dir" ]]; then
    gate_fail "corpus/profiles/ does not exist; the P1 captures live there (design §3.2)"
    gate_finish
fi

# Depth 1, because that is what the loader reads. `testkit::corpus` uses `read_dir` and
# does not recurse, so a profile in a subdirectory is invisible to every test — and a
# population built with a recursive `find` would *count* it while nothing loaded it, which
# is the dead corpus this gate is named for, hiding inside the gate itself.
mapfile -t profiles < <(find "$profiles_dir" -maxdepth 1 -type f -name '*.json' | sort)
profile_count="${#profiles[@]}"
gate_checked "$profile_count" "committed device profile(s) in corpus/profiles/"
gate_require_nonzero "$profile_count" "committed device profiles"

# The other half of that agreement: nothing deeper, where the loader would never look.
buried="$(find "$profiles_dir" -mindepth 2 -type f -name '*.json' | wc -l)"
if ((buried > 0)); then
    gate_fail "$buried profile(s) live below corpus/profiles/ in a subdirectory; the corpus loader does not recurse, so nothing will ever load them"
fi
gate_checked "$buried" "profile(s) checked for being out of the loader's reach"

# ------------------------------------------------------------------ the test population
#
# Rust sources outside the corpus directory itself. Collected once and grepped in memory,
# so the per-profile loop below does not re-walk the tree for every file.

rust_files=()
while IFS= read -r -d '' file; do
    rust_files+=("$file")
done < <(gate_rust_files)

if ((${#rust_files[@]} == 0)); then
    gate_fail "no Rust sources found; nothing could load a profile"
    gate_finish
fi

# A file *reaches* the corpus if it walks the whole directory or names a profile by stem.
# A file *replays* it if it also constructs a backend from what it reached. The two are
# computed independently: nesting the second inside the first (as an earlier version did)
# meant a tree that named every profile individually and replayed them all still failed
# claim 3, and — worse — made the claim-2 case below go red for the wrong reason.
walkers=()
replayers=()
for file in "${rust_files[@]}"; do
    reaches=0
    if grep -Eq "$walker_pattern" "$file"; then
        walkers+=("${file#"$root"/}")
        reaches=1
    else
        for profile in "${profiles[@]}"; do
            if grep -Fq "$(basename "$profile" .json)" "$file"; then
                reaches=1
                break
            fi
        done
    fi
    if ((reaches == 1)) && grep -Eq "$replay_pattern" "$file"; then
        replayers+=("${file#"$root"/}")
    fi
done

# ------------------------------------------------------------------ claim 2

if ((${#walkers[@]} > 0)); then
    gate_note "the whole corpus is walked by: ${walkers[*]}"
else
    gate_note "no test walks the whole corpus; every profile must be named individually"
fi

covered=0
for profile in "${profiles[@]}"; do
    stem="$(basename "$profile" .json)"
    if ((${#walkers[@]} > 0)); then
        covered=$((covered + 1))
        continue
    fi
    named=0
    for file in "${rust_files[@]}"; do
        if grep -Fq "$stem" "$file"; then
            named=1
            break
        fi
    done
    if ((named == 1)); then
        covered=$((covered + 1))
    else
        gate_fail "corpus/profiles/$stem.json is loaded by no test — dead corpus (design §3.2)"
    fi
done
gate_checked "$covered" "profile(s) reachable from at least one test"

# ------------------------------------------------------------------ claim 3

gate_checked "${#replayers[@]}" "test file(s) replaying the corpus through a backend"
if ((${#replayers[@]} == 0)); then
    gate_fail "no test both reaches the corpus and constructs a backend from it; parsing a profile is not replaying it, and a corpus that is only parsed proves nothing about behaviour"
else
    gate_note "the corpus is replayed by: ${replayers[*]}"
fi

gate_finish
