#!/bin/sh
# The S3 wire tier: run tests/s3_tests.rs with --include-ignored and fail unless every listed
# test ran and passed.
#
# usage: tests/s3-wire.sh PREFIX MIN
#
# PREFIX is the test-name filter (harness_, find_, write_, view_, or "" for the whole file); MIN is
# the fewest tests the selection may hold. CI's "S3 tests" step runs it over the whole file, with
# MIN equal to the file's listed count, so a test that vanished, skipped or was filtered away
# turns the step red. Run it from the repository root, with XDU_TEST_EXTENSION_DIR naming a
# directory that holds the pinned httpfs (AGENTS.md, Testing, says how to fetch one).
#
# Under --include-ignored libtest always reports "0 ignored", so that phrase proves nothing. What
# does: the passed count equals the count `--list` reports for the same filter, and that count is at
# least MIN. Cargo's exit status is read from the command itself, never through a pipe, so neither
# a missing pipefail nor a SIGPIPE from an early-exiting grep can change the verdict.
set -u

die() {
    echo "s3-wire.sh: $*" >&2
    exit 1
}

if [ "$#" -ne 2 ]; then die "usage: tests/s3-wire.sh PREFIX MIN"; fi
prefix=$1
min=$2

: "${XDU_TEST_EXTENSION_DIR:=$PWD/target/test-ext}"
export XDU_TEST_EXTENSION_DIR

if [ ! -f "$XDU_TEST_EXTENSION_DIR/httpfs.duckdb_extension" ]; then
    die "no httpfs at $XDU_TEST_EXTENSION_DIR (AGENTS.md, Testing, says how to fetch it)"
fi
# The debug binaries' default lookup. A file there would let a reader that wrongly loads httpfs
# for a local index pass every local test.
if [ -e target/lib/xdu/extensions/httpfs.duckdb_extension ]; then
    die "httpfs sits at target/lib/xdu/extensions, the default lookup; move it out"
fi
if [ ! -f tests/s3_tests.rs ]; then die "tests/s3_tests.rs is missing"; fi
# A wire test that returns early reports ok. The fixture panics instead (tests/common/s3.rs).
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

# CI's record of what ran, read from both test legs; keep its shape.
echo "s3-wire: '$prefix' $passed of $listed passed"
