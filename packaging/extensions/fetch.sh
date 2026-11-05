#!/bin/sh
# Download the pinned DuckDB extensions for the engine a release leg just built.
#
# usage: fetch.sh PROBE DEST
#
# PROBE is the extension-probe example from the same `cargo build` as the tarball binaries.
# Its manifest names the engine version, the platform and the pinned set, so nothing here
# hard-codes any of them: a leg can only fetch for the engine it is about to ship. Each file
# lands in DEST as <name>.duckdb_extension and must pass footer.sh, or the leg fails.
#
# Written for POSIX sh under `set -e`: one assertion per line, because neither a `!` command
# nor a non-final member of an `&&` list stops the script when it fails. The manifest is
# captured by assignment, so a failing probe stops the script; `for x in $(probe)` would not.
set -eu

die() {
    echo "fetch.sh: $*" >&2
    exit 1
}

if [ "$#" -ne 2 ]; then die "usage: fetch.sh PROBE DEST"; fi
probe=$1
dest=$2
if [ -z "$probe" ]; then die "empty PROBE argument"; fi
if [ -z "$dest" ]; then die "empty DEST argument"; fi
here=$(cd "$(dirname "$0")" && pwd -P)

manifest=$("$probe" manifest)

# one KEY: the value of the single KEY= line, failing on none or several.
one() {
    n=$(printf '%s\n' "$manifest" | grep -c "^$1=" || true)
    if [ "$n" -ne 1 ]; then die "manifest has $n '$1=' lines, expected 1"; fi
    printf '%s\n' "$manifest" | sed -n "s/^$1=//p"
}
engine=$(one engine)
platform=$(one platform)
if ! printf '%s\n' "$engine" | grep -Eqx 'v[0-9]+\.[0-9]+\.[0-9]+'; then die "engine '$engine' is not shaped vX.Y.Z"; fi
case $platform in
    linux_amd64 | linux_arm64 | osx_amd64 | osx_arm64) ;;
    *) die "platform '$platform' is not one a release leg builds" ;;
esac
names=$(printf '%s\n' "$manifest" | sed -n 's/^extension=//p')
if [ -z "$names" ]; then die "manifest names no extensions"; fi
if printf '%s\n' "$names" | grep -Evqx '[a-z0-9_]+'; then die "manifest names an extension outside [a-z0-9_]"; fi

mkdir -p "$dest"
for name in $names; do
    url="https://extensions.duckdb.org/$engine/$platform/$name.duckdb_extension.gz"
    echo "fetch: $url" >&2
    curl -fsSL --retry 3 -o "$dest/$name.duckdb_extension.gz" "$url"
    gunzip -f "$dest/$name.duckdb_extension.gz"
    sh "$here/footer.sh" "$dest/$name.duckdb_extension" "$engine" "$platform"
done
