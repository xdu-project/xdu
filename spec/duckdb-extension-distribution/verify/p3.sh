#!/bin/sh
# P3 gate, in the two runs TECH.md describes.
#
#   sh verify/p3.sh structural   run A: the workflow YAML only (verify/p3.py)
#   sh verify/p3.sh              run B and the phase's verify: run A's check plus the draft PR's
#                                per-leg evidence in evidence/p3-pr-run.md
#
# uv gets fresh cache and tool directories under $TMPDIR, so the gate works inside a sandbox that
# cannot write the default ones. It still needs pypi.org and files.pythonhosted.org for pyyaml.
set -eu
cd "$(dirname "$0")/../../.."
D=spec/duckdb-extension-distribution

U=$(mktemp -d "${TMPDIR:-/tmp}/xdu-p3-uv.XXXXXX")
trap 'rm -rf "$U"' EXIT
UV_CACHE_DIR="$U/cache" UV_TOOL_DIR="$U/tools" uv run --with pyyaml python "$D/verify/p3.py"

if [ "${1:-}" = structural ]; then
    echo P3-RUN-A-GREEN
    exit 0
fi

# Run B: the draft PR ran all four legs, and each leg's check.sh printed its final line.
E="$D/evidence/p3-pr-run.md"
test -f "$E"
grep -qE '^run: https://github[.]com/xdu-project/xdu/actions/runs/[0-9]+$' "$E"
sha=$(sed -n 's/^head: \([0-9a-f]\{7,40\}\)$/\1/p' "$E")
[ -n "$sha" ]
git merge-base --is-ancestor "$sha" HEAD

for leg in x86_64-unknown-linux-gnu:linux_amd64 aarch64-unknown-linux-gnu:linux_arm64 \
    x86_64-apple-darwin:osx_amd64 aarch64-apple-darwin:osx_arm64; do
    t=${leg%%:*}
    p=${leg##*:}
    n=$(grep -cE "^${t}: extension-check: ok platform=${p} engine=v[0-9]+[.][0-9]+[.][0-9]+\$" "$E" || true)
    [ "$n" -eq 1 ] || { echo "no single passing check line for ${t}" >&2; exit 1; }
done
engines=$(sed -n 's/^.*: extension-check: ok platform=[a-z0-9_]* engine=\(v[0-9.]*\)$/\1/p' "$E" | sort -u | wc -l | tr -d ' ')
[ "$engines" -eq 1 ] || { echo "legs disagree on the engine version" >&2; exit 1; }

echo P3-GATE-GREEN
