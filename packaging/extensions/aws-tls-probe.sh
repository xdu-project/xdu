#!/bin/sh
# A platform smoke check: the shipped tools find this host's CA bundle and verify AWS's real
# certificate chain with it, in both TLS stacks.
#
# usage: aws-tls-probe.sh BINDIR [EXTDIR]
#
# BINDIR holds xdu, xdu-find and xdu-view; EXTDIR, when given, is their XDU_EXTENSION_DIR
# (else the readers look beside BINDIR, as an installed tool does). Run it with HOME naming an
# empty directory: every tool runs under `env -i` with that HOME, and it must still be empty at
# the end. check.sh passes its cold HOME; the test workflow a fresh one under the runner's temp.
#
# Each tool goes to s3://xdu-tls-probe/p on AWS (us-east-1, no endpoint override) with AWS's
# documented example key pair, so no account and no secret is involved, and nothing is written:
# the writer's first request is a listing, which AWS refuses. What passes is any HTTP answer,
# since an answer means the TLS handshake verified: a status, refused credentials, or a bucket
# that does not exist. What fails is a word about a certificate or about AWS_CA_BUNDLE, or no
# answer at all ("cannot reach", retried twice). This proves the roots, not that verification is
# on: with it off AWS answers 403 too. The loopback tests (tests/s3_tests.rs, tls_) own that.
#
# Per tool it prints `aws-tls-probe: <tool>: <the line that answered>`, and once the first
# platform CA bundle present, for information only.
set -eu

die() {
    echo "aws-tls-probe: $*" >&2
    exit 1
}

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then die "usage: aws-tls-probe.sh BINDIR [EXTDIR]"; fi
bindir=$1
extdir=${2:-}
for b in xdu xdu-find xdu-view; do
    if [ ! -x "$bindir/$b" ]; then die "no executable $bindir/$b"; fi
done
if [ -n "$extdir" ] && [ ! -d "$extdir" ]; then die "no such extension directory: $extdir"; fi
: "${HOME:?HOME must name an empty directory}"
mkdir -p "$HOME"
if [ -n "$(ls -A "$HOME")" ]; then die "HOME ($HOME) is not empty"; fi
cold=$HOME

# The platform bundles the resolver looks for, in its order (src/s3.rs, PLATFORM_CA_BUNDLES).
bundle=none
for f in /etc/ssl/certs/ca-certificates.crt /etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem \
    /etc/pki/tls/certs/ca-bundle.crt /etc/ssl/ca-bundle.pem /etc/ssl/cert.pem; do
    if [ -f "$f" ]; then
        bundle=$f
        break
    fi
done
echo "aws-tls-probe: platform bundle $bundle"

s=$(mktemp -d "${TMPDIR:-/tmp}/xdu-tls-probe.XXXXXX")
trap 'rm -rf "$s"' EXIT
mkdir -p "$s/src/probe"
printf 'x\n' > "$s/src/probe/f"
locator=s3://xdu-tls-probe/p

# probe TOOL ARGS...: run TOOL against AWS until it answers, at most three times.
probe() {
    tool=$1
    shift
    attempt=1
    while :; do
        # An empty XDU_EXTENSION_DIR means the default lookup beside BINDIR.
        if out=$(env -i PATH="$PATH" HOME="$cold" XDU_EXTENSION_DIR="$extdir" \
            AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE \
            AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
            AWS_REGION=us-east-1 \
            "$bindir/$tool" "$@" < /dev/null 2>&1 > /dev/null); then
            die "$tool succeeded against $locator with the example key pair"
        fi
        if printf '%s\n' "$out" | grep -qi 'certificate'; then die "$tool: $out"; fi
        if printf '%s\n' "$out" | grep -qF 'AWS_CA_BUNDLE'; then die "$tool: $out"; fi
        answer=$(printf '%s\n' "$out" | grep -E 'HTTP [0-9]{3}|refused the credentials|does not exist at the S3 endpoint' | head -n 1 || true)
        if [ -n "$answer" ]; then
            echo "aws-tls-probe: $tool: $answer"
            return 0
        fi
        if printf '%s\n' "$out" | grep -qF 'cannot reach' && [ "$attempt" -lt 3 ]; then
            attempt=$((attempt + 1))
            sleep 5
            continue
        fi
        die "$tool: no answer from AWS: $out"
    done
}

probe xdu-find -i "$locator" --count
probe xdu-view -i "$locator"
probe xdu "$s/src" -o "$locator"

if [ -n "$(ls -A "$cold")" ]; then die "files appeared under HOME: $(cd "$cold" && find . | head -n 20)"; fi
