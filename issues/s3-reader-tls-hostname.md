---
status: unshaped
kind: fix
appetite: big
---

# The S3 readers' TLS checks differ from the writer's: the CN fallback and two CA-bundle limits

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu-find` and `xdu-view` accept an `https` S3 endpoint whose certificate names the endpoint's
host only in its subject common name (CN), when its subject alternative names (SAN) name another
host or it has no SAN at all. `xdu` refuses the same endpoint. So one resolved endpoint gets two
levels of trust from the two directions, the class `s3-index-target` review cycle 3's finding 1
was.

Two operator-bundle limits of the same split ride with it. The resolver's root rule
(`is_self_signed_root` in `bundle_certificates`, `src/s3.rs`) stands in for the readers' OpenSSL
chain check, and it is narrower in one direction and wider in the other:

- **A root self-signed with SHA-1 does not count.** A root counts only when the writer's ring
  verifies its self-signature, so an `AWS_CA_BUNDLE` whose only roots are SHA-1-signed (or have
  P-521 keys) is refused as "holds no root CA certificate", although both TLS stacks accept a chain
  that ends at it. It fails closed.
- **An intermediate beside an unrelated root passes.** The rule asks for some self-signed root, not
  that the file's CA certificates chain to one. The writer's webpki anchors at the intermediate and
  writes the index; the readers' OpenSSL cannot, and refuses the endpoint as untrusted.

The readers' TLS client is the pinned `httpfs`'s httplib, which `engine::open_s3_connection_from`
pins with `SET httpfs_client_implementation = 'httplib'` (`DUCKDB_TLS_PINNING` in `src/s3.rs`).
httplib checks the host against the SAN and, failing that, against the subject CN. The writer's
rustls client (webpki, through `reqwest`'s `tls_certs_only` in `src/s3.rs`) checks the SAN alone,
as RFC 6125 and the CA/Browser Forum rules require.

The exposure needs a CA in the trust source to issue such a certificate. Public CAs must list the
CN among the SANs, so with a platform bundle this takes a mis-issued certificate. A private CA
named by `AWS_CA_BUNDLE` can issue one. The readers then send signed requests to that server, with
the session token when one is set, and return the index it serves.

**Reproduction** (review cycle 4, `spec/s3-index-target/REVIEW.md`, finding 1):

- A TLS stand-in serving a minimal S3, its certificate `subject=CN=127.0.0.1` with SAN
  `DNS:wrong.example`, signed by the CA `AWS_CA_BUNDLE` names:
  - `xdu-find --count` printed the count and exited 0;
  - `xdu-view` made its LIST, marker HEAD and GET, and ranged chunk GETs, then stopped only at
    terminal setup;
  - `xdu -o` exited 1: `invalid peer certificate: certificate not valid for name "127.0.0.1";
    certificate is only valid for DnsName("wrong.example")`.
- A certificate with CN `127.0.0.1` and no SAN split the same way.
- The project's own wire test PKI with the wrong-name leaf's CN changed from `xdu test endpoint`
  to `127.0.0.1` turned `tls_a_self_signed_endpoint_is_refused_by_all_three_tools` red with
  "xdu-find: the tool succeeded".

**Reproduction** (review cycle 5, `spec/s3-index-target/REVIEW.md`, findings 1–2):

- DigiCert Global Root CA (`sha1WithRSAEncryption`, issuer equal to subject, `openssl verify
  -CAfile r.pem r.pem` gives `OK`) cut from `/etc/ssl/cert.pem` as `AWS_CA_BUNDLE`: `xdu-find`,
  `xdu-view` and `xdu -o` each exit 1 before any request with "holds no root CA certificate". With
  a SHA-256 root appended, the resolver passes. On the wire, a SHA-1 root made with `openssl`
  signing a SHA-256 leaf for `IP:127.0.0.1` is refused alone by all three tools with no connection,
  and answers once an unrelated SHA-256 root sits beside it.
- The wire test PKI's `root-and-intermediate.pem` rebuilt from the unrelated CA and the
  intermediate turns `tls_a_ca_bundle_without_a_root_is_refused_and_one_with_it_answers` red at
  the readers: `xdu` exits 0 and writes the marker, then `xdu-find` fails with "the S3 endpoint's
  TLS certificate is not trusted". `/etc/ssl/cert.pem` plus the intermediate splits the same way;
  the root plus the intermediate answers in all three tools.

**The red-first test already exists.** `tests/s3_tests.rs`'s
`tls_a_leaf_naming_the_host_only_in_its_cn_is_refused_by_xdu_alone` drives both leaves (CN
`127.0.0.1` with SAN `DNS:wrong.example`, and CN `127.0.0.1` with no SAN). It asserts today's
split: `xdu` refuses under the refusal rule, and `xdu-find` and `xdu-view` answer. The fix flips its
reader half to the same refusal, so that test is this cycle's red-first: rewrite the reader half
to assert the refusal and watch it fail on today's tree.

Neither bundle limit has a red-first test yet: the fix adds one per limit, each failing on the
tree before it.

## Why it was deferred

The fallback sits inside the pinned extension's client, and `s3-index-target`'s non-goals bar
changing the pinned extension set or bumping `duckdb`. The human ruled option 1 at review cycle 4
(GOAL Clarification, 2026-11-06): land the S3 pilot with this recorded as R11's one named exception
and `invariants.md` §5's known exception, and **commit to this fix cycle**. New with S3 support; not
present on `main`. Review cycle 5 found the two bundle limits in the root rule that cycle 4's
remediation added; the human ruled "record and land" (GOAL Clarification, 2026-11-06), recording
both in R11 as operator-bundle limits until this cycle lands.

## Outcome / vision

All three tools apply one host-name rule, the SAN alone, to every `https` S3 endpoint, and one
trust decision over the CA file: a SHA-1 root anchors a chain in every direction, and an
intermediate without its root anchors one in none. The R11 exception and limits and the §5
exception are gone, no reader request reaches a server whose certificate the writer would refuse,
and `xdu` never writes an index the readers refuse.

## Candidate routes

A spike settles which route holds; neither is chosen here.

1. **An in-process loopback relay.** The reader starts a relay on `127.0.0.1` inside its own
   process. `httpfs` speaks plain HTTP to it through its `http_proxy` setting, and the relay opens
   TLS to the real endpoint with the writer's rustls client and the one trust source. Both
   directions then verify with one verifier, which also closes both bundle limits: the resolver's
   root rule can then match the one verifier, or go. The spike must answer:
   - **SigV4 and `Host`.** httpfs signs the `Host` header it sends. With the endpoint marked
     `USE_SSL false` so httpfs speaks HTTP, does it still sign the host and port the TLS endpoint
     sees (default ports differ: 80 against 443)? Does the relay forward `Host` and every
     `x-amz-*` header byte-for-byte, never re-signing?
   - **Proxy form.** Does httplib send absolute-form request targets or `CONNECT` through
     `http_proxy`, for both virtual-hosted and path-style URLs?
   - **Keep-alive.** httplib reuses connections. The relay must hold a client connection open
     across requests, map each request to a pooled TLS connection, and never mix two requests'
     bytes.
   - **Ranged reads.** `Range` and `Content-Range` must pass untouched, and responses must stream
     without buffering an object, or R3's bytes-received bound breaks.
   - **Refusal words.** A certificate the relay refuses must still reach the reader as "TLS
     certificate is not trusted", with no request sent upstream.
   - **The proxy setting.** It is the one `issues/s3-proxy-support.md` would give an operator's
     proxy. The two must compose, or that seed must say how they will.
2. **An upstream change** to httpfs, or to the httplib it vendors: an option that turns off the
   CN fallback, then a bump of the pinned extension. Switching httpfs to its curl client is not
   enough. curl ignores `ca_cert_file` (`s3-index-target` P6 probes) and falls back on the CN when
   a certificate has no SAN of the matching type. This route leaves both bundle limits open, so
   it would need a root rule that matches OpenSSL's chain building on its own.

## Sketch of the acceptance criteria

- **R1** — WHEN an `https` S3 endpoint presents a certificate whose SAN does not name the endpoint's
  host, `xdu-find` and `xdu-view` SHALL refuse it as `xdu` does, whatever its subject CN says,
  with the certificate refusal on stderr and no request reaching the endpoint.
- **R2** — The wire test `tls_a_leaf_naming_the_host_only_in_its_cn_is_refused_by_xdu_alone`
  SHALL be rewritten so that all three tools refuse both CN-only leaves, renamed to say so, and
  shown failing on the tree before the fix.
- **R3** — **Acceptance is removing the exception:** `s3-index-target` R11's recorded exception and
  `invariants.md` §5's "One known exception" bullet, with its `AGENTS.md` mirror and the man-page
  and README "Known limitation" sentences, SHALL be deleted in the same change.
- **R4** — WHEN `AWS_CA_BUNDLE` holds a root self-signed with SHA-1 and the endpoint's chain ends
  at it, all three tools SHALL answer as over plain HTTP; WHEN it holds an intermediate whose own
  root is absent, beside an unrelated root, `xdu` SHALL refuse the endpoint as the readers do,
  before it writes. Each SHALL have a wire test shown failing on the tree before the fix, and R11's
  two operator-bundle limits, with their README and man-page sentences, SHALL be deleted in the
  same change.
- **R5** — Every existing `tls_` wire test and R3's bytes-received bound SHALL still hold, and no
  route SHALL add a way to turn verification off.

## Notes

- Related: `spec/s3-index-target/` (GOAL R11 and its 2026-11-06 Clarifications, PLAN §7.6, REVIEW
  cycle 4 finding 1 and cycle 5 findings 1–2); `issues/s3-proxy-support.md`, which shares the
  readers' `http_proxy` setting.
- The two bundle limits were first recorded at PLAN §7.6, then in R11 after review cycle 5
  (findings 1–2, MEDIUM). Route 1 closes them with the rest; route 2 does not.
- Found by: `s3-index-target` review cycle 4 (finding 1, CRITICAL) and review cycle 5 (findings
  1–2, MEDIUM).
