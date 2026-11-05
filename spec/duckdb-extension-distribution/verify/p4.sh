#!/bin/sh
# P4 gate: install.sh places the extensions where an installed tool finds them with no
# configuration, including the two layouts most likely to fool it, and the contract is recorded.
#
# Drives the real install.sh through a curl shim that serves a locally assembled tarball and its
# SHA256SUMS. Needs the network once, for extensions.duckdb.org. Written at plan time.
set -eu
cd "$(dirname "$0")/../../.."

T=$(rustc -vV | sed -n 's/^host: //p')
TD=${CARGO_TARGET_DIR:-target}
cargo build --release --locked --target "$T" --bins --example extension-probe
P="$TD/$T/release/examples/extension-probe"

S=$(mktemp -d "${TMPDIR:-/tmp}/xdu-p4.XXXXXX")
trap 'rm -rf "$S"' EXIT
mkdir -p "$S/staging/bin" "$S/staging/share/man/man1" "$S/srv" "$S/shim"
# A release tarball always carries its man pages, and install.sh copies share/man/man1/*.1.
printf '.TH XDU 1\n' > "$S/staging/share/man/man1/xdu.1"
for b in xdu xdu-find xdu-view xdu-rm; do cp "$TD/$T/release/$b" "$S/staging/bin/"; done
sh packaging/extensions/fetch.sh "$P" "$S/staging/lib/xdu/extensions"
A="xdu-vtest-$T.tar.gz"
tar czf "$S/srv/$A" -C "$S/staging" bin share lib
(cd "$S/srv" && { sha256sum "$A" 2> /dev/null || shasum -a 256 "$A"; } > SHA256SUMS)

cat > "$S/shim/curl" << 'EOF'
#!/bin/sh
out=''; url=''
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        -*) shift ;;
        *) url=$1; shift ;;
    esac
done
cp "$SHIM_DIR/${url##*/}" "$out"
EOF
chmod +x "$S/shim/curl"

install_to() {
    PATH="$S/shim:$PATH" SHIM_DIR="$S/srv" XDU_VERSION=vtest XDU_PREFIX="$S/prefix" \
        XDU_INSTALL="$1" sh install.sh > /dev/null
}

load_cold() {
    mkdir -p "$2"
    (unset XDU_EXTENSION_DIR; HOME="$2" "$1/extension-probe" load)
    [ -z "$(find "$2" -mindepth 1 | head -n 1)" ] || { echo "cold HOME written: $2" >&2; exit 1; }
}

# Binaries outside the prefix: extensions follow the binaries, not XDU_PREFIX.
install_to "$S/elsewhere/bin"
test -f "$(cd "$S/elsewhere" && pwd -P)/lib/xdu/extensions/httpfs.duckdb_extension"
cp "$P" "$S/elsewhere/bin/"
load_cold "$S/elsewhere/bin" "$S/cold1"

# The upgrade path: a second install over the first replaces the file, and it still loads.
install_to "$S/elsewhere/bin"
cp "$P" "$S/elsewhere/bin/"
load_cold "$S/elsewhere/bin" "$S/cold1b"

# A symlinked install directory: extensions land where the canonical executable looks.
mkdir -p "$S/real/bin"
ln -s "$S/real/bin" "$S/linkbin"
install_to "$S/linkbin"
test -f "$(cd "$S/real" && pwd -P)/lib/xdu/extensions/httpfs.duckdb_extension"
cp "$P" "$S/linkbin/"
load_cold "$S/linkbin" "$S/cold2"

sh -n install.sh

# The contract and the deferrals are written down where the next cycle will look.
grep -qF 'lib/xdu/extensions' AGENTS.md
grep -qF 'lib/xdu/extensions' .agents/factory/invariants.md
grep -qF 'XDU_EXTENSION_DIR' AGENTS.md
grep -qF 'XDU_EXTENSION_DIR' README.md
grep -qF 'lib/xdu/extensions' hpccm/xdu.py
grep -qF '/usr/local/lib/xdu/extensions' Dockerfile
grep -qF 'packaging/extensions/check.sh' Dockerfile
grep -qF 'test -f /usr/local/lib/xdu/extensions/httpfs.duckdb_extension' Dockerfile
grep -qF 'lib/xdu/extensions' issues/native-os-packages-deb-rpm.md
grep -qF 'XDU_EXTENSION_DIR' issues/s3-index-target.md
grep -qF 'test -f' issues/hpccm-extension-guard.md
grep -qF 'issues/hpccm-extension-guard.md' ROADMAP.md

echo P4-GATE-GREEN
