---
status: unshaped
kind: fix
appetite: small
---

# Reach an AWS bucket whose name holds a dot

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

The bucket rule `S3Locator::parse` enforces (`bucket_name_problem` in `src/s3.rs`) admits `.`, and
the man pages of `xdu`, `xdu-find` and `xdu-view` say dots are allowed. With no endpoint override,
though, all three tools address AWS virtual-hosted over `https`:

- the writer through `client_endpoint_url` (`https://<bucket>.s3.<region>.amazonaws.com`);
- the readers through the `URL_STYLE` of `vhost` they bind for AWS.

AWS documents that its wildcard certificate `*.s3.<region>.amazonaws.com` covers one label, so a
bucket such as `my.bucket` puts two labels under the wildcard and fails verification.

Since `s3-index-target` R11, every tool verifies the server, so `s3://my.bucket/idx` against AWS is
refused by all three. The error blames the certificate, "TLS certificate is not trusted … it must
chain to a CA in the file AWS_CA_BUNDLE names". It does not blame the bucket's name, so an operator
is sent after the wrong cause.

**Not reproduced.** The loopback endpoint forces path style, and the review host had no route
beyond loopback. It fails closed. Review cycle 3 dropped the same candidate on those grounds, and
cycle 4 recorded it because R11 now makes the readers refuse it too.

## Why it was deferred

Found as LOW and PLAUSIBLE at `s3-index-target` review cycle 4 (finding 3), against a contract whose
acceptance is the loopback endpoint alone; real-provider behaviour is outside it. The foreman's
brief after the human's ruling seeded it rather than widen the remediation. New with S3 support;
not present on `main`.

## Outcome / vision

A dotted bucket either works against AWS or is refused before any request with an error that names
the dot and the remedy. It is never refused as an untrusted certificate.

## Sketch of the acceptance criteria

- **R1** — WHEN the endpoint is AWS's own and the bucket name holds `.`, each tool SHALL either
  address the bucket path-style in both directions, or refuse before any request with an error
  that names the dot and the remedy (an endpoint override, or a bucket without dots), never the
  certificate. Which one is settled at shaping.
- **R2** — IF path style is chosen, THEN the writer (`object_store`'s virtual-hosted switch) and the
  readers (`URL_STYLE`) SHALL take it from one resolver decision, so they address the same host and
  path. A wire test SHALL show both directions agree on a dotted bucket's request targets.
- **R3** — The man pages SHALL say what a dotted bucket does against AWS.

## Notes

- **Path style is the candidate.** It keeps the bucket out of the host, which is also what
  `issues/s3-legacy-bucket-names.md` proposes for capitals and `_`. The two can share one
  addressing decision.
- AWS still serves path-style requests to existing buckets but has deprecated path style for new
  ones. Shaping should check AWS's current guidance on dotted buckets before choosing.
- Related: `spec/s3-index-target/` (REVIEW cycle 4 finding 3; GOAL R11);
  `issues/s3-legacy-bucket-names.md`.
- Found by: `s3-index-target` review cycle 4 (finding 3, LOW, plausible).
