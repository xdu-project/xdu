#!/bin/sh
# Scope gate for R6 and R7: the fix changes exactly one line of production code, writes no new
# value into SQL text, and leaves the frozen copy of main's local statements untouched.
#
# usage: sh spec/xdu-view-prefix-bytes-vs-chars/verify/scope.sh   (from the repository root)
#
# - No file under src/ other than src/view_query.rs differs from 7689a92, and none is new.
# - In src/view_query.rs above `#[cfg(test)]`, ignoring comment lines, the only difference from
#   7689a92 is `prefix.len()` -> `prefix.chars().count()` on the prefix_len line. Any other change to
#   a statement builder (a new literal, a second bound value, a quoting helper) fails here, so R7
#   is held structurally rather than by guessing what a widening would look like.
# - `old_directory_parts`, the frozen copy of what xdu-view sent, is byte-identical to 7689a92's, so
#   the R6 pin still compares against main's own text.
# - The pin's case count and the offset unit test's literal are present as live code.
set -u

base=7689a92

die() {
    echo "scope.sh: $*" >&2
    exit 1
}

if [ ! -f Cargo.toml ] || [ ! -f src/view_query.rs ]; then die "run from the repository root"; fi
git rev-parse --verify --quiet "$base^{commit}" >/dev/null || die "no commit $base"
mkdir -p target/scope || die "cannot make target/scope"

# Other source files: unchanged and no new ones.
git diff --quiet "$base" -- src ':(exclude)src/view_query.rs' || die "a file under src/ other than view_query.rs changed"
st=$(git status --porcelain --untracked-files=all -- src) || die "git status failed"
case $st in
'' | ' M src/view_query.rs' | 'M  src/view_query.rs' | 'MM src/view_query.rs') ;;
*) die "unexpected source state: $st" ;;
esac

# The production part of view_query.rs, comment lines dropped, before and after.
strip() { sed -e '/^#\[cfg(test)\]/,$d' -e '/^[[:space:]]*\/\//d'; }
git show "$base:src/view_query.rs" >target/scope/base.rs || die "git show failed"
strip <target/scope/base.rs >target/scope/base.prod
strip <src/view_query.rs >target/scope/work.prod
test -s target/scope/base.prod && test -s target/scope/work.prod || die "empty production text"
diff target/scope/base.prod target/scope/work.prod | grep '^[<>]' >target/scope/prod.diff
printf '%s\n' '<         let prefix_len = prefix.len();' '>         let prefix_len = prefix.chars().count();' >target/scope/prod.want
cmp -s target/scope/prod.diff target/scope/prod.want || {
    cat target/scope/prod.diff >&2
    die "production view_query.rs differs from $base by more than the offset line"
}

# The frozen copy is main's, unchanged.
frozen() { sed -n '/^    fn old_directory_parts(/,/^    }$/p'; }
frozen <target/scope/base.rs >target/scope/base.frozen
frozen <src/view_query.rs >target/scope/work.frozen
test -s target/scope/base.frozen || die "old_directory_parts not found in $base"
cmp -s target/scope/base.frozen target/scope/work.frozen || die "old_directory_parts changed"

grep -qE '^[[:space:]]*assert_eq!\(checked, 3 \* 7 \* 4\);' src/view_query.rs || die "the pin does not count 3 * 7 * 4 cases"
grep -qF 'substr(path, 9 + 1)' src/view_query.rs || die "the offset test does not state the code-point offset"

echo "scope: one production line changed; frozen copy intact"
