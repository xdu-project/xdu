#!/bin/sh
# Red gate for R5: this branch's local regression tests fail on main at 052bbc7, each at its own
# first assertion. A green phase gate cannot show that a test detects the defect; this runs the
# branch's tests/ against main's src/ and requires every named test to run and fail with its token.
#
# usage: sh spec/hive-partition-columns-find-rm/verify/red.sh   (from the repository root)
#
# Each run extracts 052bbc7 into a fresh directory under the untracked target/red-trees/, copies
# the branch's tests/ over it, and builds into target/red-target, never target/: CARGO_BIN_EXE_*
# is fixed at compile time to the target dir it was built in, so the branch's own gates never run
# a binary built from main. --no-fail-fast, so a red find_tests binary does not stop rm_tests
# and view_tests from running. Nothing is deleted. The first run compiles the bundled DuckDB
# there.
set -u

base=052bbc7

# test-binary  test-name  token its first assertion's message carries
cases='find_tests find_reads_a_key_value_partition_as_a_bare_name key=value partition:
find_tests find_reads_an_index_under_a_key_value_directory_as_a_bare_one key=value index directory:
rm_tests rm_reads_a_key_value_partition_as_a_bare_name key=value partition:
rm_tests rm_reads_an_index_under_a_key_value_directory_as_a_bare_one key=value index directory:
view_tests view_reads_a_key_value_partition_as_a_bare_name key=value partition:
view_tests view_reads_an_index_under_a_key_value_directory_as_a_bare_one key=value index directory:'

die() {
    echo "red.sh: $*" >&2
    exit 1
}

root=$PWD
if [ ! -f "$root/Cargo.toml" ] || [ ! -d "$root/tests" ]; then die "run from the repository root"; fi
git rev-parse --verify --quiet "$base^{commit}" >/dev/null || die "no commit $base"
mkdir -p "$root/target/red-trees" || die "cannot make target/red-trees"
work=$(mktemp -d "$root/target/red-trees/base.XXXXXX") || die "cannot make a fresh tree directory"
src=$work/src
mkdir "$src" || die "cannot make $src"
log=$work/test.log

git archive --format=tar -o "$work/tree.tar" "$base" || die "git archive $base failed"
tar -xf "$work/tree.tar" -C "$src" || die "cannot extract the tree"
# The extracted tree must be the defect under test, or a failure there would mean nothing.
if grep -qF 'hive_partitioning' "$src/src/lib.rs" "$src/src/bin/xdu-rm.rs"; then
    die "$src already turns Hive detection off; it is not the defect"
fi
cp -R "$root/tests/." "$src/tests/" || die "cannot copy tests/"

# git archive stamps files with the commit time, older than any build, so a previous tree's
# library could answer. Stamp every file now, and require below that xdu was compiled.
find "$src" -type f -exec touch {} + || die "cannot stamp the tree's files"

names=$(printf '%s\n' "$cases" | awk '{print $2}')
# shellcheck disable=SC2086
(cd "$src" && CARGO_TARGET_DIR="$root/target/red-target" cargo test --locked --no-fail-fast \
    --test find_tests --test rm_tests --test view_tests -- --exact $names) >"$log" 2>&1
rc=$?
grep -qF "Compiling xdu v" "$log" || die "xdu was not compiled for $base, so another tree's build answered (see $log)"
if [ "$rc" -eq 0 ]; then die "the named tests pass on $base (or do not exist), so they do not detect it (see $log)"; fi

printf '%s\n' "$cases" | while read -r bin name token; do
    grep -qF "test $name ... FAILED" "$log" || die "$name did not run and fail on $base (see $log)"
    # The test's own failure section, from its header to the next one.
    awk -v h="---- $name stdout ----" '
        $0 == h { on = 1; next }
        on && /^---- .* stdout ----$/ { exit }
        on { print }
    ' "$log" >"$work/$name.section"
    grep -qF "$token" "$work/$name.section" || die "$name failed on $base, but not with \"$token\" (see $log)"
    echo "red: $name ($bin) fails on $base with \"$token\""
done || exit 1

echo "red: all six local regression tests fail on $base"
