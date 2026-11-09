#!/bin/sh
# Scope gate for R4 and R7: under src/, the fix is the Hive option and nothing else.
#
# usage: sh spec/hive-partition-columns-find-rm/verify/scope.sh   (from the repository root)
#
# - No file under src/ is added or removed relative to main at 052bbc7.
# - Every file under src/, its test module included, equals 052bbc7's once comment lines are
#   dropped, every ", hive_partitioning = false" is deleted and all whitespace is removed. So the
#   only change to code is inserting that option, and the pinned statement texts in the unit tests
#   moved by exactly that much: updated, not relaxed (R4), and no new value reached SQL text (R4,
#   invariants.md §5). Comment lines may change freely (the partitions_sql comment, R7).
# - The production part of each file (above its first #[cfg(test)]) carries the option exactly
#   where a reader statement is built: twice in src/lib.rs (parquet_source, local and S3 arms),
#   once in src/bin/xdu-rm.rs, never in src/view_query.rs, which takes its source from
#   parquet_source.
# - Every production read_parquet( outside src/select.rs is immediately followed by its glob and
#   the option, so a reader that bypasses parquet_source and the option fails here.
# - The partitions_sql comment no longer claims a Hive reading yields no partition column (R7).
set -u

base=052bbc7
opt=', hive_partitioning = false'

die() {
    echo "scope.sh: $*" >&2
    exit 1
}

if [ ! -f Cargo.toml ] || [ ! -f src/lib.rs ]; then die "run from the repository root"; fi
git rev-parse --verify --quiet "$base^{commit}" >/dev/null || die "no commit $base"
mkdir -p target/scope || die "cannot make target/scope"

# No file added or removed under src/ (tracked or not).
git diff --name-only --diff-filter=ADR "$base" -- src >target/scope/added-removed || die "git diff failed"
git ls-files --others --exclude-standard -- src >>target/scope/added-removed || die "git ls-files failed"
if [ -s target/scope/added-removed ]; then
    cat target/scope/added-removed >&2
    die "a file under src/ was added, removed or renamed"
fi

normalize() {
    # Drop comment lines, delete the option, remove all whitespace.
    sed -e '/^[[:space:]]*\/\//d' | OPT=$opt awk '{
        while ((i = index($0, ENVIRON["OPT"])) > 0) $0 = substr($0, 1, i - 1) substr($0, i + length(ENVIRON["OPT"]))
        print
    }' | tr -d '[:space:]'
}
production() { sed -e '/^#\[cfg(test)\]/,$d'; }

changed=0
for f in $(git ls-files -- src); do
    git show "$base:$f" >target/scope/base.rs 2>/dev/null || die "$f is not in $base"
    normalize <target/scope/base.rs >target/scope/base.norm
    normalize <"$f" >target/scope/work.norm
    test -s target/scope/work.norm || die "$f normalizes to nothing"
    cmp -s target/scope/base.norm target/scope/work.norm || die "$f differs from $base by more than the Hive option and comments"
    cmp -s target/scope/base.rs "$f" || changed=$((changed + 1))
done
test "$changed" -ge 3 || die "only $changed file(s) under src/ changed; expected lib.rs, xdu-rm.rs and view_query.rs"

count() { production <"$1" | tr -d '[:space:]' | grep -o 'hive_partitioning=false' | wc -l | tr -d ' '; }
test "$(count src/lib.rs)" -eq 2 || die "src/lib.rs production code carries the option $(count src/lib.rs) times, not 2"
test "$(count src/bin/xdu-rm.rs)" -eq 1 || die "src/bin/xdu-rm.rs carries the option $(count src/bin/xdu-rm.rs) times, not 1"
test "$(count src/view_query.rs)" -eq 0 || die "src/view_query.rs builds its own source"

# Every production read_parquet( outside select.rs is followed by its glob and the option.
for f in $(git ls-files -- 'src/*.rs' 'src/bin/*.rs'); do
    [ "$f" = src/select.rs ] && continue
    production <"$f" | sed -e '/^[[:space:]]*\/\//d' | tr -d '[:space:]' >target/scope/prod.flat
    total=$(grep -o 'read_parquet(' target/scope/prod.flat | wc -l | tr -d ' ')
    good=$(grep -oE "read_parquet\\(('\\{\\}'|\\?),hive_partitioning=false" target/scope/prod.flat | wc -l | tr -d ' ')
    test "$total" -eq "$good" || die "$f has a read_parquet( without the Hive option ($good of $total)"
done

if grep -qF 'would yield no partition column' src/view_query.rs; then
    die "the partitions_sql comment still says a Hive reading yields no partition column"
fi

echo "scope: src/ differs from $base only by the Hive option and comments"
