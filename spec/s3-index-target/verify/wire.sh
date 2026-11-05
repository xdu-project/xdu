#!/bin/sh
# Run one family of S3 wire tests and fail unless every listed test ran and passed.
#
# usage: wire.sh PREFIX MIN
#
# PREFIX is the test-name filter (harness_, find_, write_, view_, or "" for the whole file); MIN is
# the fewest tests the family may hold. Written at plan time so a build phase is not graded by a
# gate it wrote itself. Both each phase's verify and the CI S3 step run this one script.
#
# Under --include-ignored libtest always reports "0 ignored", so that phrase proves nothing. What
# does: the passed count equals the count `--list` reports for the same filter, and that count is at
# least MIN. Cargo's exit status is read from the command itself, never through a pipe, so neither
# a missing pipefail nor a SIGPIPE from an early-exiting grep can change the verdict.
set -u

die() {
    echo "wire.sh: $*" >&2
    exit 1
}

if [ "$#" -ne 2 ]; then die "usage: wire.sh PREFIX MIN"; fi
prefix=$1
min=$2

: "${XDU_TEST_EXTENSION_DIR:=$PWD/target/test-ext}"
XDU_TEST_S3_REQUIRED=1
export XDU_TEST_EXTENSION_DIR XDU_TEST_S3_REQUIRED

if [ ! -f "$XDU_TEST_EXTENSION_DIR/httpfs.duckdb_extension" ]; then
    die "no httpfs at $XDU_TEST_EXTENSION_DIR (see TECH.md, Shared verify prerequisite)"
fi
# The debug binaries' default lookup. A file there would let a reader that wrongly loads httpfs
# for a local index pass every local test.
if [ -e target/lib/xdu/extensions/httpfs.duckdb_extension ]; then
    die "httpfs sits at target/lib/xdu/extensions, the default lookup; move it out"
fi
if [ ! -f tests/s3_tests.rs ]; then die "tests/s3_tests.rs is missing"; fi
# A wire test that returns early reports ok. The fixture panics instead (TECH.md conventions).
if grep -nE '^[[:space:]]*return([[:space:]]*;|[[:space:]]+Ok\(\(\)\))' tests/s3_tests.rs; then
    die "an early return in tests/s3_tests.rs would pass a test that did not run"
fi

mkdir -p target
# Fixed names under the untracked target/: each run overwrites the last, and nothing is deleted.
name=${prefix:-all}
log=target/wire-$name.log
lst=target/wire-$name.list

cargo test --locked --test s3_tests "$prefix" -- --list --include-ignored >"$lst" 2>/dev/null
rc=$?
if [ "$rc" -ne 0 ]; then cat "$lst" >&2; die "listing failed (exit $rc)"; fi
listed=$(grep -c ': test$' "$lst")

cargo test --locked --test s3_tests "$prefix" -- --include-ignored >"$log" 2>&1
rc=$?
cat "$log" >&2
if [ "$rc" -ne 0 ]; then die "cargo test exited $rc"; fi

passed=$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed; 0 failed;.*/\1/p' "$log")
if [ -z "$passed" ]; then die "no clean summary line"; fi
if [ "$listed" -lt "$min" ]; then die "'$prefix' lists $listed tests, fewer than $min"; fi
if [ "$passed" -ne "$listed" ]; then die "'$prefix' passed $passed of $listed listed"; fi

# Run B of each phase reads this line from both CI test legs; keep its shape.
echo "s3-wire: '$prefix' $passed of $listed passed"
