---
status: unshaped
kind: fix
appetite: big
---

# The S3 readers accept a certificate whose CN alone names the endpoint

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu-find` and `xdu-view` accept an `https` S3 endpoint whose certificate names the endpoint's
host only in its subject common name (CN), when its subject alternative names (SAN) name another
host or it has no SAN at all. `xdu` refuses the same endpoint. So one resolved endpoint gets two
levels of trust from the two directions, the class `s3-index-target` review cycle 3's finding 1
was.

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

**The red-first test already exists.** `tests/s3_tests.rs`'s
`tls_a_leaf_naming_the_host_only_in_its_cn_is_refused_by_xdu_alone` drives both leaves (CN
`127.0.0.1` with SAN `DNS:wrong.example`, and CN `127.0.0.1` with no SAN). It asserts today's
split: `xdu` refuses under the refusal rule, and `xdu-find` and `xdu-view` answer. The fix flips its
reader half to the same refusal, so that test is this cycle's red-first: rewrite the reader half
to assert the refusal and watch it fail on today's tree.

## Why it was deferred

The fallback sits inside the pinned extension's client, and `s3-index-target`'s non-goals bar
changing the pinned extension set or bumping `duckdb`. The human ruled option 1 at review cycle 4
(GOAL Clarification, 2026-11-06): land the S3 pilot with this recorded as R11's one named exception
and `invariants.md` §5's known exception, and **commit to this fix cycle**. New with S3 support; not
present on `main`.

## Outcome / vision

All three tools apply one host-name rule, the SAN alone, to every `https` S3 endpoint. The R11
exception and the §5 exception are gone, and no reader request reaches a server whose certificate
the writer would refuse.

## Candidate routes

A spike settles which route holds; neither is chosen here.

1. **An in-process loopback relay.** The reader starts a relay on `127.0.0.1` inside its own
   process. `httpfs` speaks plain HTTP to it through its `http_proxy` setting, and the relay opens
   TLS to the real endpoint with the writer's rustls client and the one trust source. Both
   directions then verify with one verifier, which also closes the narrower trust-source edge in
   *Notes*. The spike must answer:
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
   a certificate has no SAN of the matching type.

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
- **R4** — Every existing `tls_` wire test and R3's bytes-received bound SHALL still hold, and no
  route SHALL add a way to turn verification off.

## Notes

- Related: `spec/s3-index-target/` (GOAL R11 and its 2026-11-06 Clarification, PLAN §7.6, REVIEW
  cycle 4 finding 1); `issues/s3-proxy-support.md`, which shares the readers' `http_proxy` setting.
- A narrower edge of the same class, recorded at PLAN §7.6: an `AWS_CA_BUNDLE` holding a root and,
  beside it, an intermediate whose own root is absent. The writer anchors at that intermediate;
  the readers' OpenSSL cannot. Route 1 closes it with the rest; route 2 does not.
- Found by: `s3-index-target` review cycle 4 (finding 1, CRITICAL).
