#!/bin/sh
# Prove a release tarball's DuckDB extensions load, from the extracted layout, under a cold HOME.
#
# usage: check.sh TARBALL PROBE
#
# PROBE is the extension-probe example from the same `cargo build` as the tarball binaries.
# A release leg runs this on the exact file it is about to upload, before the upload, so a
# tarball whose extensions do not match its engine or platform, or do not load, is never
# published. The last line on success is
#
#     extension-check: ok platform=<platform> engine=<engine>
#
# and nothing follows it: that line is the per-leg evidence read back from each CI log.
#
# Every probe and tool run is `env -i PATH="$PATH" HOME="$cold"`. An exported
# XDU_EXTENSION_DIR, a proxy variable httpfs would honour, or a build image's LD_LIBRARY_PATH
# must not decide the verdict, and the cold HOME must still be empty at the end.
#
# Written for POSIX sh under `set -e`: one assertion per line, because neither a `!` command
# nor a non-final member of an `&&` list stops the script when it fails. The manifest is
# captured by assignment, so a failing probe stops the script; `for x in $(probe)` would not.
set -eu

die() {
    echo "check.sh: $*" >&2
    exit 1
}

if [ "$#" -ne 2 ]; then die "usage: check.sh TARBALL PROBE"; fi
tarball=$1
probe=$2
if [ ! -f "$tarball" ]; then die "no such tarball: $tarball"; fi
if [ ! -f "$probe" ]; then die "no such probe: $probe"; fi
here=$(cd "$(dirname "$0")" && pwd -P)

# Physical path, so the exe-relative paths the probe prints compare equal to the ones built here.
s=$(mktemp -d "${TMPDIR:-/tmp}/xdu-extcheck.XXXXXX")
trap 'rm -rf "$s"' EXIT
s=$(cd "$s" && pwd -P)
x="$s/x"
cold="$s/home"
mkdir "$x" "$cold"

cold_run() {
    env -i PATH="$PATH" HOME="$cold" "$@"
}

tar xzf "$tarball" -C "$x"
for b in xdu xdu-find xdu-view xdu-rm xdu-cp xdu-mv; do
    if [ ! -x "$x/bin/$b" ]; then die "tarball has no executable bin/$b"; fi
done
cp "$probe" "$x/bin/extension-probe"
chmod +x "$x/bin/extension-probe"

manifest=$(cold_run "$x/bin/extension-probe" manifest)

# one KEY: the value of the single KEY= line, failing on none or several.
one() {
    n=$(printf '%s\n' "$manifest" | grep -c "^$1=" || true)
    if [ "$n" -ne 1 ]; then die "manifest has $n '$1=' lines, expected 1"; fi
    printf '%s\n' "$manifest" | sed -n "s/^$1=//p"
}
engine=$(one engine)
platform=$(one platform)
source_id=$(one source_id)
if ! printf '%s\n' "$engine" | grep -Eqx 'v[0-9]+\.[0-9]+\.[0-9]+'; then die "engine '$engine' is not shaped vX.Y.Z"; fi
case $platform in
    linux_amd64 | linux_arm64 | osx_amd64 | osx_arm64) ;;
    *) die "platform '$platform' is not one a release leg builds" ;;
esac
if [ -z "$source_id" ]; then die "manifest has an empty source_id"; fi
names=$(printf '%s\n' "$manifest" | sed -n 's/^extension=//p')
if [ -z "$names" ]; then die "manifest names no extensions"; fi
if printf '%s\n' "$names" | grep -Evqx '[a-z0-9_]+'; then die "manifest names an extension outside [a-z0-9_]"; fi
first=$(printf '%s\n' "$names" | head -n 1)

# The shipped set is exactly the pinned set: nothing missing, nothing extra.
ext="$x/lib/xdu/extensions"
if [ ! -d "$ext" ]; then die "tarball has no lib/xdu/extensions/"; fi
expected=$(printf '%s\n' "$names" | sed 's/$/.duckdb_extension/' | LC_ALL=C sort)
shipped=$(cd "$ext" && ls -A | LC_ALL=C sort)
if [ "$shipped" != "$expected" ]; then die "lib/xdu/extensions/ holds [$shipped], the pinned set is [$expected]"; fi
for name in $names; do
    sh "$here/footer.sh" "$ext/$name.duckdb_extension" "$engine" "$platform"
done

# The probe resolves the directory beside its own executable, as an installed tool does.
loaded=$(cold_run "$x/bin/extension-probe" load)
printf '%s\n' "$loaded"
n_loaded=$(printf '%s\n' "$loaded" | grep -c '^loaded=' || true)
n_names=$(printf '%s\n' "$names" | grep -c . || true)
if [ "$n_loaded" -ne "$n_names" ]; then die "probe loaded $n_loaded extensions, the pinned set has $n_names"; fi
for name in $names; do
    if ! printf '%s\n' "$loaded" | grep -qxF "loaded=$name $ext/$name.duckdb_extension"; then die "$name did not load from $ext"; fi
done

# The engine-version tie. source_id is constant across every build of one engine version, so
# this proves the shipped readers carry the probe's engine version, not that they came from
# the same build; that rests on the workflow building both in one cargo invocation.
for b in xdu-find xdu-view xdu-rm xdu-cp xdu-mv; do
    if ! grep -a -q -F "$source_id" "$x/bin/$b"; then die "bin/$b does not carry engine source_id $source_id"; fi
done

# Every shipped binary must execute, not merely carry the right bytes: a loader, libc or C++
# runtime floor the runner cannot meet shows only when the binary runs. `--version` needs no
# index and no terminal, so it runs the same way for the TUI as for the batch tools; stdin is
# /dev/null and stdout a pipe.
for b in xdu xdu-find xdu-view xdu-rm xdu-cp xdu-mv; do
    if ! v=$(cold_run "$x/bin/$b" --version < /dev/null 2>&1); then die "bin/$b --version did not run: $v"; fi
    if ! printf '%s\n' "$v" | grep -Eqx "$b [0-9]+\.[0-9]+\.[0-9]+.*"; then die "bin/$b --version printed '$v', expected '$b <version>'"; fi
done

# The shipped tools themselves, under the same cold HOME.
tree="$s/tree"
mkdir -p "$tree/user1" "$tree/user2"
printf 'a\n' > "$tree/user1/a.log"
printf 'b\n' > "$tree/user1/b.log"
printf 'c\n' > "$tree/user2/c.txt"
cold_run "$x/bin/xdu" --apparent-size -o "$s/index" "$tree" > /dev/null
count=$(cold_run "$x/bin/xdu-find" -i "$s/index" --count)
if [ "$count" != 3 ]; then die "bin/xdu-find --count printed '$count', expected 3"; fi
dry=$(cold_run "$x/bin/xdu-rm" -i "$s/index" --pattern '*.log' --dry-run)
if ! printf '%s\n' "$dry" | grep -qF '2 file(s) would be deleted'; then die "bin/xdu-rm --dry-run did not match the two .log files: $dry"; fi
if [ ! -f "$tree/user1/a.log" ]; then die "bin/xdu-rm --dry-run deleted a file"; fi
mkdir "$s/stage"
dry=$(cold_run "$x/bin/xdu-cp" -i "$s/index" --pattern '*.log' --into "$s/stage" --root "$tree" --dry-run)
if ! printf '%s\n' "$dry" | grep -qF '2 file(s) would be copied'; then die "bin/xdu-cp --dry-run did not match the two .log files: $dry"; fi
if [ -n "$(ls -A "$s/stage")" ]; then die "bin/xdu-cp --dry-run wrote under --into"; fi

# The shipped tools find this host's CA bundle and verify AWS's real chain with it, in both TLS
# stacks: a platform smoke check, not acceptance (the loopback tests own that). It needs the
# extracted lib/ still in place, so it runs before expect_absent moves it aside, and under the
# cold HOME, which must stay empty.
cold_run sh "$here/aws-tls-probe.sh" "$x/bin"

# An absent extension fails naming the exact path, and nothing is fetched in its place.
# expect_absent DIR [OVERRIDE]: the probe must fail naming DIR/<first>.duckdb_extension.
expect_absent() {
    want="$1/$first.duckdb_extension"
    if [ "$#" -eq 2 ]; then
        if err=$(cold_run XDU_EXTENSION_DIR="$2" "$x/bin/extension-probe" load 2>&1); then die "probe loaded with $2 as the extension directory"; fi
    else
        if err=$(cold_run "$x/bin/extension-probe" load 2>&1); then die "probe loaded with lib/ removed"; fi
    fi
    if ! printf '%s\n' "$err" | grep -qF "does not download"; then die "absent-extension error lacks the loader's wording: $err"; fi
    if ! printf '%s\n' "$err" | grep -qF "$want"; then die "absent-extension error does not name $want: $err"; fi
}
mkdir "$s/empty"
expect_absent "$s/empty" "$s/empty"
expect_absent "$s/missing" "$s/missing"
if [ -e "$s/missing" ]; then die "the missing override directory was created"; fi
mv "$x/lib" "$s/lib.aside"
expect_absent "$ext"

# Nothing above may have written under HOME: no extension cache, no secrets directory.
if [ -n "$(ls -A "$cold")" ]; then die "files appeared under the cold HOME: $(cd "$cold" && find . | head -n 20)"; fi

echo "extension-check: ok platform=$platform engine=$engine"
