#!/bin/sh
# P2 gate: the host's own release leg, end to end, plus the controls that must go red.
#
# Written at plan time, before the scripts it drives exist, so the build cannot grade itself.
# Needs the network once, for extensions.duckdb.org. Run from anywhere inside the worktree.
set -eu
cd "$(dirname "$0")/../../.."

T=$(rustc -vV | sed -n 's/^host: //p')
TD=${CARGO_TARGET_DIR:-target}
cargo build --release --locked --target "$T" --bins --example extension-probe
P="$TD/$T/release/examples/extension-probe"
case $P in /*) ;; *) P="$(pwd -P)/$P" ;; esac

S=$(mktemp -d "${TMPDIR:-/tmp}/xdu-p2.XXXXXX")
trap 'rm -rf "$S"' EXIT
mkdir -p "$S/staging/bin" "$S/staging/share/man/man1"
for b in xdu xdu-find xdu-view xdu-rm; do cp "$TD/$T/release/$b" "$S/staging/bin/"; done

# The leg as the workflow runs it: fetch, assemble, check.
sh packaging/extensions/fetch.sh "$P" "$S/staging/lib/xdu/extensions"
tar czf "$S/good.tar.gz" -C "$S/staging" bin share lib
out=$(sh packaging/extensions/check.sh "$S/good.tar.gz" "$P")
printf '%s\n' "$out"
# The last line is the per-leg evidence P3's run B reads from each CI log.
last=$(printf '%s\n' "$out" | tail -n 1)
E0=$("$P" manifest | sed -n 's/^engine=//p')
PL0=$("$P" manifest | sed -n 's/^platform=//p')
[ "$last" = "extension-check: ok platform=$PL0 engine=$E0" ]
# A hostile environment must not change the verdict: check.sh runs everything under env -i.
XDU_EXTENSION_DIR="$S/nowhere" http_proxy='http://127.0.0.1:9' https_proxy='http://127.0.0.1:9' \
    sh packaging/extensions/check.sh "$S/good.tar.gz" "$P"

E=$("$P" manifest | sed -n 's/^engine=//p')
PL=$("$P" manifest | sed -n 's/^platform=//p')
N=$("$P" manifest | grep -c '^extension=')
# One test per line: under `set -e` a failure inside an && list does not stop the script.
[ -n "$E" ]
[ -n "$PL" ]
[ "$N" -ge 1 ]
f="$S/staging/lib/xdu/extensions/httpfs.duckdb_extension"
sh packaging/extensions/footer.sh "$f" "$E" "$PL" > /dev/null

# A relative XDU_EXTENSION_DIR is taken as given; the loader resolves it against the working
# directory, and the probe must accept a load from there. The probe's own unit tests hold the
# other side: a path outside the directory, or a sibling sharing its name prefix, stays refused.
mkdir -p "$S/rel/cwd/ext" "$S/rel/home"
cp "$S/staging/lib/xdu/extensions/"*.duckdb_extension "$S/rel/cwd/ext/"
relcwd=$(cd "$S/rel/cwd" && pwd -P)
relout=$(cd "$relcwd" && env -i PATH="$PATH" HOME="$S/rel/home" XDU_EXTENSION_DIR=ext "$P" load)
printf '%s\n' "$relout"
printf '%s\n' "$relout" | grep -qxF "loaded=httpfs $relcwd/ext/httpfs.duckdb_extension"
[ -z "$(ls -A "$S/rel/home")" ]
ut=$(cargo test --locked --example extension-probe 2>&1)
printf '%s\n' "$ut" | grep -qE 'test result: ok\. [1-9][0-9]* passed'
printf '%s\n' "$ut" | grep -qxF 'test tests::a_load_outside_the_dir_is_refused ... ok'
printf '%s\n' "$ut" | grep -qxF 'test tests::relative_dir_matches_its_absolute_load_path ... ok'

red() { echo "P2 control passed when it must fail: $1" >&2; exit 1; }

# footer.sh must refuse each rewritten field, a short file, a non-extension, a wrong engine and
# empty arguments. Offsets count back from the end of the file (research/01).
sz=$(wc -c < "$f" | tr -d ' ')
tamper() { # tamper NAME OFFSET-FROM-END TEXT
    cp "$f" "$S/$1"
    printf '%s' "$3" | dd of="$S/$1" bs=1 seek=$((sz - $2)) conv=notrunc 2> /dev/null
    if sh packaging/extensions/footer.sh "$S/$1" "$E" "$PL" > /dev/null 2>&1; then red "tampered $1"; fi
}
tamper bad-platform 320 'bogus_platform'
tamper bad-version 352 'v9.9.9'
tamper bad-magic 288 '9'
head -c 400 "$f" > "$S/short"
if sh packaging/extensions/footer.sh "$S/short" "$E" "$PL" > /dev/null 2>&1; then red 'short file'; fi
if sh packaging/extensions/footer.sh "$f" 'v0.0.0' "$PL" > /dev/null 2>&1; then red 'wrong engine'; fi
if sh packaging/extensions/footer.sh "$S/staging/bin/xdu" "$E" "$PL" > /dev/null 2>&1; then red 'not an extension'; fi
if sh packaging/extensions/footer.sh "$f" '' '' > /dev/null 2>&1; then red 'empty arguments'; fi

# check.sh must refuse a tarball with no extensions, one whose extension set differs from the
# manifest, one whose xdu-view cannot execute, and one carrying a tampered file.
tar czf "$S/no-lib.tar.gz" -C "$S/staging" bin share
if sh packaging/extensions/check.sh "$S/no-lib.tar.gz" "$P" > /dev/null 2>&1; then red 'tarball without lib/'; fi
cp -R "$S/staging" "$S/extra"
cp "$f" "$S/extra/lib/xdu/extensions/unpinned.duckdb_extension"
tar czf "$S/extra.tar.gz" -C "$S/extra" bin share lib
if sh packaging/extensions/check.sh "$S/extra.tar.gz" "$P" > /dev/null 2>&1; then red 'extension outside the pinned set'; fi
# check.sh must run every shipped binary, not only test its mode bits and grep its bytes: an
# xdu-view that carries the engine source_id but cannot execute must fail the leg, and fail it
# for that reason.
cp -R "$S/staging" "$S/negview"
sid=$("$P" manifest | sed -n 's/^source_id=//p')
[ -n "$sid" ]
printf '#!/nonexistent/interpreter\n%s\n' "$sid" > "$S/negview/bin/xdu-view"
chmod +x "$S/negview/bin/xdu-view"
grep -qF "$sid" "$S/negview/bin/xdu-view"
if "$S/negview/bin/xdu-view" --version > /dev/null 2>&1; then red 'the xdu-view stand-in executed'; fi
tar czf "$S/negview.tar.gz" -C "$S/negview" bin share lib
if err=$(sh packaging/extensions/check.sh "$S/negview.tar.gz" "$P" 2>&1); then red 'an xdu-view that cannot execute'; fi
if ! printf '%s\n' "$err" | grep -qF 'bin/xdu-view --version did not run'; then
    echo "P2 control failed, but not on the unexecutable xdu-view: $err" >&2
    exit 1
fi
cp "$S/bad-platform" "$f"
tar czf "$S/bad.tar.gz" -C "$S/staging" bin share lib
if sh packaging/extensions/check.sh "$S/bad.tar.gz" "$P" > /dev/null 2>&1; then red 'tampered extension'; fi

echo P2-GATE-GREEN
