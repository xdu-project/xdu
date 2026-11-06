#!/bin/sh
# Red gate: run this branch's tests/ against a tree where the fix is absent or wrong, and require
# that one named test runs and fails on one named assertion there. A green phase gate alone cannot
# show that a test detects the defect (GOAL R5: "fail on main 7689a92 and pass with the fix"), and
# a test that stops at its first listing can be red on main while never reaching the deep,
# decomposed, U+FFFD or 4-byte directories R3 names. The mutants close that gap: each one is the
# fixed source with the offset miscounted only for one kind of character, so the test goes red only
# if its walk really lists a directory below such a name.
#
# usage: sh spec/xdu-view-prefix-bytes-vs-chars/verify/red.sh TREE TOKEN TEST CARGO_TEST_ARGS...
#
#   TREE   base            the source of main at 7689a92 (bytes, the defect itself)
#          mutant-utf16    this branch's source, offset in UTF-16 units (wrong only past a 4-byte
#                          scalar such as U+1D11E)
#          mutant-nomarks  this branch's source, combining marks U+0300..U+036F not counted (wrong
#                          only past a decomposed name)
#          mutant-nofffd   this branch's source, U+FFFD not counted (wrong only past a lossy name)
#   TOKEN  text the failing assertion's message must carry, e.g. "local list full names under"
#   TEST   the test's full name as libtest prints it
#   CARGO_TEST_ARGS go to `cargo test --locked` and must select TEST
#
# Run from the repository root after the fix is in the working tree (the mutants copy it). Each run
# extracts into a fresh directory under the untracked target/red-trees/, so no file from an earlier
# run survives into this one; nothing is deleted. Every tree builds into target/red-target, never
# target/: CARGO_BIN_EXE_* is fixed at compile time to the target dir it was built in, so the
# branch's own gates never run a binary built from a red tree. The first run compiles the bundled
# DuckDB once for that directory.
set -u

base=7689a92
fixed='let prefix_len = prefix.chars().count();'

die() {
    echo "red.sh: $*" >&2
    exit 1
}

if [ "$#" -lt 4 ]; then die "usage: red.sh TREE TOKEN TEST CARGO_TEST_ARGS..."; fi
tree=$1
token=$2
name=$3
shift 3

case $tree in
base) mutant= ;;
mutant-utf16) mutant='let prefix_len = prefix.encode_utf16().count();' ;;
mutant-nomarks) mutant="let prefix_len = prefix.chars().filter(|c| !('\\u{300}'..='\\u{36f}').contains(c)).count();" ;;
mutant-nofffd) mutant="let prefix_len = prefix.chars().filter(|c| *c != '\\u{FFFD}').count();" ;;
*) die "unknown tree $tree" ;;
esac

root=$PWD
if [ ! -f "$root/Cargo.toml" ] || [ ! -d "$root/tests" ]; then die "run from the repository root"; fi
mkdir -p "$root/target/red-trees" || die "cannot make target/red-trees"
work=$(mktemp -d "$root/target/red-trees/$tree.XXXXXX") || die "cannot make a fresh tree directory"
src=$work/src
mkdir "$src" || die "cannot make $src"
log=$work/test.log

# Extract to a file first so a failing `git archive` is seen, not hidden behind tar's status.
if [ "$tree" = base ]; then
    git rev-parse --verify --quiet "$base^{commit}" >/dev/null || die "no commit $base"
    git archive --format=tar -o "$work/tree.tar" "$base" || die "git archive $base failed"
else
    git archive --format=tar -o "$work/tree.tar" HEAD || die "git archive HEAD failed"
fi
tar -xf "$work/tree.tar" -C "$src" || die "cannot extract the tree"

if [ "$tree" = base ]; then
    # The extracted tree must be the defect under test, or a pass here would mean nothing.
    grep -qF 'let prefix_len = prefix.len();' "$src/src/view_query.rs" ||
        die "$src is not the byte-count source of $base"
else
    # This branch's working tree, uncommitted fix included, then the one-line mutation.
    for p in src examples Cargo.toml Cargo.lock; do
        cp -R "$root/$p" "$src/" || die "cannot copy $p"
    done
    grep -qF "$fixed" "$src/src/view_query.rs" || die "the fix line is not in src/view_query.rs"
    FIXED=$fixed MUTANT=$mutant awk '{
        i = index($0, ENVIRON["FIXED"])
        if (i) { print substr($0, 1, i - 1) ENVIRON["MUTANT"]; n++ } else print
    } END { if (n != 1) exit 1 }' "$root/src/view_query.rs" >"$src/src/view_query.rs" ||
        die "the fix line is not there exactly once"
    grep -qF "$mutant" "$src/src/view_query.rs" || die "the mutation did not apply"
fi
mkdir -p "$src/tests" && cp -R "$root/tests/." "$src/tests/" || die "cannot copy tests/"

# Wire tests read the pinned httpfs from here; the path must not depend on the working directory.
: "${XDU_TEST_EXTENSION_DIR:=$root/target/test-ext}"
case $XDU_TEST_EXTENSION_DIR in
/*) ;;
*) XDU_TEST_EXTENSION_DIR=$root/$XDU_TEST_EXTENSION_DIR ;;
esac
export XDU_TEST_EXTENSION_DIR

(cd "$src" && CARGO_TARGET_DIR="$root/target/red-target" cargo test --locked "$@") >"$log" 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then die "$name passes on $tree, so it does not detect it (see $log)"; fi
grep -qF "test $name ... FAILED" "$log" || die "$name did not run and fail on $tree (see $log)"
grep -qF "$token" "$log" || die "$name failed on $tree, but not with \"$token\" (see $log)"

echo "red: $name fails on $tree with \"$token\""
