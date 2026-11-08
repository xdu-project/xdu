#!/bin/sh
# The cross-filesystem tier: run tests/mv_xfs_tests.rs with --include-ignored and fail unless
# every listed test ran and passed.
#
# usage: tests/xfs-wire.sh PREFIX MIN
#
# PREFIX is the test-name filter ("" for the whole file); MIN is the fewest tests the selection
# may hold. Run it from the repository root with XDU_TEST_XFS_DIR naming a writable directory on
# another filesystem (AGENTS.md, Testing, says how to make one). CI's "XFS tests" step runs it
# over the whole file, with MIN equal to the file's listed count, so a test that vanished,
# skipped or was filtered away turns the step red.
#
# The script does not check XDU_TEST_XFS_DIR itself: the fixture in tests/mv_xfs_tests.rs panics
# when it is unset, and when a probe rename into it does not fail with EXDEV, so the reason a
# run fails is the fixture's own and a run without a second filesystem can never pass.
#
# Under --include-ignored libtest always reports "0 ignored", so that phrase proves nothing. What
# does: the passed count equals the count `--list` reports for the same filter, and that count is at
# least MIN. Cargo's exit status is read from the command itself, never through a pipe, so neither
# a missing pipefail nor a SIGPIPE from an early-exiting grep can change the verdict.
set -u

die() {
    echo "xfs-wire.sh: $*" >&2
    exit 1
}

if [ "$#" -ne 2 ]; then die "usage: tests/xfs-wire.sh PREFIX MIN"; fi
prefix=$1
min=$2

if [ ! -f tests/mv_xfs_tests.rs ]; then die "tests/mv_xfs_tests.rs is missing"; fi
# A test that returns early reports ok. The fixture panics instead.
if grep -nE '^[[:space:]]*return([[:space:]]*;|[[:space:]]+Ok\(\(\)\))' tests/mv_xfs_tests.rs; then
    die "an early return in tests/mv_xfs_tests.rs would pass a test that did not run"
fi

mkdir -p target
# Fixed names under the untracked target/: each run overwrites the last, and nothing is deleted.
name=${prefix:-all}
log=target/xfs-wire-$name.log
lst=target/xfs-wire-$name.list

cargo test --locked --test mv_xfs_tests "$prefix" -- --list --include-ignored >"$lst" 2>/dev/null
rc=$?
if [ "$rc" -ne 0 ]; then cat "$lst" >&2; die "listing failed (exit $rc)"; fi
listed=$(grep -c ': test$' "$lst")

cargo test --locked --test mv_xfs_tests "$prefix" -- --include-ignored >"$log" 2>&1
rc=$?
cat "$log" >&2
if [ "$rc" -ne 0 ]; then die "cargo test exited $rc"; fi

passed=$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed; 0 failed;.*/\1/p' "$log")
if [ -z "$passed" ]; then die "no clean summary line"; fi
if [ "$listed" -lt "$min" ]; then die "'$prefix' lists $listed tests, fewer than $min"; fi
if [ "$passed" -ne "$listed" ]; then die "'$prefix' passed $passed of $listed listed"; fi

# CI's record of what ran, read from both test legs; keep its shape.
echo "xfs-wire: '$prefix' $passed of $listed passed with XDU_TEST_XFS_DIR=${XDU_TEST_XFS_DIR:-}"
