---
status: unshaped
kind: feature
appetite: small
---

# Reach S3 buckets whose names carry capitals or `_`

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu -o s3://bucket/prefix`, `xdu-find -i s3://…` and `xdu-view -i s3://…` refuse a bucket whose
name holds an uppercase letter or `_`. The refusal comes before any request is sent and names
the character, not the bucket. A bucket with such a name is not reachable from xdu at all.

Such buckets exist. Several S3-compatible stores still let an operator create them, and AWS
keeps serving legacy buckets created before its naming rules tightened. An operator whose
index would live in one has to move it to a bucket with a conforming name first.

The refusal follows from how the locator is checked. `S3Locator::parse` (`bucket_name_problem`
in `src/s3.rs`) holds the bucket to S3's current naming rules as an allow-list: 3 to 63
characters, lowercase letters, digits, `.` and `-`, a letter or digit at each end, no `..`.
The allow-list exists so that nothing in a locator can reach the URL authority. With
virtual-hosted addressing the bucket *is* part of the host, and a character outside the list
could add a port, end the authority, introduce user information, or read differently to two
URL parsers, so the writer and the readers could reach different hosts from one value.

These names could still be served by path-style requests, where the bucket is the first path
segment under the endpoint and never part of the host. Both directions already switch to path
style when an endpoint override is set; for AWS they address virtual-hosted, which cannot carry
these names.

## Why it was deferred

The foreman ruled at `s3-index-target` review cycle 2, run B (2026-11-05), to keep the refusal
and file this seed. The allow-list was the cycle's fix for a locator reaching a host no setting
named, and widening it needs its own proof that a wider set stays out of the authority in both
directions. Refusing is safe: it fails loud, before any request. New with S3 support; not
present on `main`.

## Outcome / vision

A bucket with capitals or `_` in its name works as an index location whenever requests to it are
path-style, and every reader answers from it exactly as from a conforming bucket. No character of
the bucket can reach the URL authority under either addressing style.

## Sketch of the acceptance criteria

- **R1** — WHEN requests are path-style, the locator parser SHALL accept a bucket name that also
  holds uppercase ASCII letters and `_`, and SHALL still refuse every character that can end,
  extend or re-split a URL authority (`:`, `/`, `?`, `#`, `@`, `\`, `%`, whitespace, controls and
  anything outside ASCII).
- **R2** — WHEN requests are virtual-hosted, the parser SHALL keep today's allow-list, and the
  refusal SHALL say that the name needs path-style addressing.
- **R3** — The writer and the readers SHALL address the same host and the same path for such a
  bucket; a wire test SHALL show `xdu -o`, `xdu-find` and `xdu-view` round-trip an index in a
  bucket with capitals and `_` against a loopback endpoint.
- **R4** — A predicate test SHALL walk every character through the widened parser and both
  directions' URL builders and assert that none outside the allow-list reaches the authority.
- **R5** — WHEN AWS is the endpoint, whether to offer path-style addressing for legacy buckets
  (AWS has deprecated path style for new buckets) SHALL be settled at shaping, and if offered it
  SHALL be one setting that reaches both directions.

## Notes

- Related: `spec/s3-index-target/` (TECH review cycle 2 triage, F3, and the ruling recorded there);
  `issues/s3-non-ascii-partition-names.md`, the other key/name character question for S3.
- Unprobed: how the pinned `httpfs` lists and globs a path-style bucket whose name has capitals or
  `_`.
- Found by: `s3-index-target` review cycle 2 (F3's allow-list).
