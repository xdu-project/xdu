#!/bin/sh
# Backport gate for R6: the fix's product-and-test diff applies without conflict onto v0.6.0 with
# #36 (0547790) cherry-picked, and builds and passes there, without anything #37 added.
#
# usage: sh spec/hive-partition-columns-find-rm/verify/backport.sh   (from the repository root)
#
# - The restricted diff is `git diff 052bbc7 -- src tests` of the working tree (the gate runs
#   before the phase commit). Every other changed path must be main-only: AGENTS.md,
#   .agents/factory/invariants.md, .github/workflows/test.yaml, ROADMAP.md, issues/ and this
#   cycle's spec/. Anything else (a man page, the README) fails here, so a doc that would belong
#   in the restricted diff is decided on purpose, not dropped.
# - No untracked file under src/ or tests/: `git diff` would leave it out of the restricted diff.
# - No added line names what #37 introduced (named here; the build at v0.6.0 proves it as well).
# - A scratch worktree, detached at v0.6.0, takes `git cherry-pick --no-commit 0547790`, then
#   `git apply` of the restricted diff, with no fuzz and no three-way merge, then
#   `cargo test --locked` in full, built into target/red-target. The six local regression tests
#   and the four statement pins must be listed ok there.
# - Informational: whether this change's .github/workflows/test.yaml hunk also applies there.
# - The worktree is reset and removed with `git worktree remove` (never --force); if that
#   refuses, the gate fails and says so. No branch is created; nothing is pushed.
set -u

base=052bbc7
tag=v0.6.0
pick=0547790

die() {
    echo "backport.sh: $*" >&2
    exit 1
}

root=$PWD
if [ ! -f "$root/Cargo.toml" ] || [ ! -d "$root/tests" ]; then die "run from the repository root"; fi
for rev in "$base" "$tag" "$pick"; do
    git rev-parse --verify --quiet "$rev^{commit}" >/dev/null || die "no commit $rev"
done
mkdir -p "$root/target/backport" || die "cannot make target/backport"
work=$(mktemp -d "$root/target/backport/run.XXXXXX") || die "cannot make a run directory"

# Paths outside the restricted diff must be main-only.
git diff --name-only "$base" -- . ':(exclude)src' ':(exclude)tests' >"$work/other" || die "git diff failed"
git ls-files --others --exclude-standard -- . ':(exclude)src' ':(exclude)tests' ':(exclude)target' >>"$work/other"
while read -r f; do
    case $f in
    AGENTS.md | .agents/factory/invariants.md | .github/workflows/test.yaml | ROADMAP.md) ;;
    issues/* | spec/hive-partition-columns-find-rm/*) ;;
    *) die "$f changed: it is neither src/, tests/ nor main-only; decide whether it backports" ;;
    esac
done <"$work/other"

untracked=$(git ls-files --others --exclude-standard -- src tests)
[ -z "$untracked" ] || die "untracked under src/ or tests/ (not in the restricted diff): $untracked"

git diff --binary "$base" -- src tests >"$work/fix.diff" || die "git diff failed"
test -s "$work/fix.diff" || die "the restricted diff is empty"
if grep -nE '^\+.*(select::|atomic_fs|relocate|build_index_block_size|append_bytes|run_cp|run_mv|run_with_stdin|xdu-cp|xdu-mv|XduCpArgs|XduMvArgs|SelectArgs|cp_tests|mv_tests)' "$work/fix.diff"; then
    die "an added line names something #37 introduced"
fi

wt=$work/tree
git worktree add --quiet --detach "$wt" "$tag" || die "cannot add a worktree at $tag"
cleanup() {
    (cd "$wt" && git reset --quiet --hard HEAD && { git cherry-pick --quit 2>/dev/null || :; })
    git worktree remove "$wt" || echo "backport.sh: git worktree remove $wt refused; remove it by hand" >&2
}

(cd "$wt" && git cherry-pick --no-commit "$pick") >"$work/pick.log" 2>&1 || {
    cleanup
    die "#36 ($pick) does not cherry-pick cleanly onto $tag (see $work/pick.log)"
}
(cd "$wt" && git apply --check "$work/fix.diff" && git apply "$work/fix.diff") >"$work/apply.log" 2>&1 || {
    cleanup
    die "the restricted diff does not apply onto $tag + $pick (see $work/apply.log)"
}

git diff "$base" -- .github/workflows/test.yaml >"$work/ci.diff"
if [ -s "$work/ci.diff" ]; then
    if (cd "$wt" && git apply --check "$work/ci.diff") >/dev/null 2>&1; then
        echo "backport: info: the test.yaml hunk also applies onto $tag + $pick"
    else
        echo "backport: info: the test.yaml hunk does NOT apply onto $tag + $pick (main-only)"
    fi
fi

(cd "$wt" && CARGO_TARGET_DIR="$root/target/red-target" cargo test --locked) >"$work/test.log" 2>&1
rc=$?
grep -qF "Compiling xdu v" "$work/test.log" || { cleanup; die "xdu was not compiled in the backport tree (see $work/test.log)"; }
[ "$rc" -eq 0 ] || { cleanup; die "cargo test fails on $tag + $pick + the fix (see $work/test.log)"; }
for t in find_reads_a_key_value_partition_as_a_bare_name \
    find_reads_an_index_under_a_key_value_directory_as_a_bare_one \
    rm_reads_a_key_value_partition_as_a_bare_name \
    rm_reads_an_index_under_a_key_value_directory_as_a_bare_one \
    view_reads_a_key_value_partition_as_a_bare_name \
    view_reads_an_index_under_a_key_value_directory_as_a_bare_one \
    tests::test_parquet_source_keeps_local_sql_byte_for_byte \
    tests::test_parquet_source_binds_the_s3_glob \
    view_query::tests::local_view_sql_is_byte_for_byte_what_xdu_view_sent \
    view_query::tests::s3_view_sql_binds_the_glob_and_the_directory_prefix; do
    grep -qF "test $t ... ok" "$work/test.log" || { cleanup; die "$t is not ok on $tag + $pick (see $work/test.log)"; }
done

cleanup
[ -d "$wt" ] && die "the backport worktree was not removed"
echo "backport: the restricted diff applies onto $tag + $pick and cargo test passes there"
