#!/bin/sh
# Run one spike probe.
#
# Cargo only compiles integration tests under tests/, and these probes must NOT live
# there: they need a MinIO endpoint, they reach the network, and several deliberately
# print failures rather than asserting. Putting them in tests/ would make `cargo test`
# depend on a container. So the probe is copied in for the length of one run and removed
# after -- scratch this script creates and disposes of itself, which is the case
# AGENTS.md keeps on `rm` rather than `del`: there is nothing to recover, and trashing a
# copy of a committed file just fills the trash.
#
#   sh bench/spikes/run.sh extension
#   FIXTURE=big sh bench/spikes/run.sh fixtures
#   SQL="SELECT 1" sh bench/spikes/run.sh query
#
# Everything after the probe name is passed to the test binary:
#   sh bench/spikes/run.sh layout -- --nocapture --test-threads=1
set -eu

probe="${1:-}"
if [ -z "$probe" ]; then
    echo "usage: sh bench/spikes/run.sh <probe> [cargo test args...]" >&2
    echo "probes: $(cd "$(dirname "$0")/probes" && ls *.rs | sed 's/\.rs$//' | tr '\n' ' ')" >&2
    exit 2
fi
shift

root=$(cd "$(dirname "$0")/../.." && pwd)
src="$root/bench/spikes/probes/$probe.rs"
dst="$root/tests/$probe.rs"

[ -f "$src" ] || { echo "no such probe: $src" >&2; exit 2; }
# Never clobber a real test file. tests/ is committed; a name collision means the probe
# needs renaming, not that the suite should be overwritten.
[ -e "$dst" ] && { echo "refusing to overwrite existing $dst" >&2; exit 2; }

cleanup() { rm -f "$dst"; }
trap cleanup EXIT INT TERM

cp "$src" "$dst"
echo "--- $probe (copied to tests/, removed on exit) ---"
cd "$root"
cargo test --test "$probe" -- --nocapture "$@"
