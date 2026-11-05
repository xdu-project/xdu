#!/bin/sh
# Check a DuckDB extension file against the engine version and platform it must match.
#
# usage: footer.sh FILE ENGINE PLATFORM
#
# Every extension file ends in a 512-byte footer: 256 bytes of metadata, then a 256-byte
# signature. The metadata is eight 32-byte NUL-padded fields stored last-field-first, so each
# field sits at a fixed offset from the end of the file. The magic field is checked first: a
# future footer layout then fails on the magic instead of comparing whatever bytes happen to
# sit where the platform used to be. DuckDB refuses the same mismatches at LOAD; this check
# fails a release leg before anything is packed or uploaded.
#
# Written for POSIX sh under `set -e`: one assertion per line, because neither a `!` command
# nor a non-final member of an `&&` list stops the script when it fails.
set -eu
# Bytes, not characters: under a UTF-8 locale BSD `tr` aborts on the first invalid sequence, so
# a file that is not an extension would fail with "Illegal byte sequence" instead of on its magic.
LC_ALL=C
export LC_ALL

die() {
    echo "footer.sh: $*" >&2
    exit 1
}

if [ "$#" -ne 3 ]; then die "usage: footer.sh FILE ENGINE PLATFORM"; fi
file=$1
engine=$2
platform=$3
if [ -z "$file" ]; then die "empty FILE argument"; fi
if [ -z "$engine" ]; then die "empty ENGINE argument"; fi
if [ -z "$platform" ]; then die "empty PLATFORM argument"; fi
if [ ! -f "$file" ]; then die "not a regular file: $file"; fi

size=$(wc -c < "$file" | tr -d ' ')
if [ "$size" -lt 512 ]; then die "$file is $size bytes, shorter than the 512-byte footer"; fi

# field OFFSET: the 32-byte field starting OFFSET bytes before the end of the file.
field() {
    tail -c "$1" "$file" | head -c 32 | tr -d '\000'
}

magic=$(field 288)
if [ "$magic" != 4 ]; then die "$file: footer magic is not 4; not an extension, or an unknown footer layout"; fi
found_platform=$(field 320)
if [ "$found_platform" != "$platform" ]; then die "$file: built for platform '$found_platform', expected '$platform'"; fi
found_engine=$(field 352)
if [ "$found_engine" != "$engine" ]; then die "$file: built for engine '$found_engine', expected '$engine'"; fi
extension_version=$(field 384)
abi=$(field 416)

if command -v sha256sum > /dev/null 2>&1; then
    sum=$(sha256sum "$file")
else
    sum=$(shasum -a 256 "$file")
fi
sum=${sum%% *}

echo "footer: ok $file engine=$found_engine platform=$found_platform extension_version=$extension_version abi=$abi sha256=$sum"
