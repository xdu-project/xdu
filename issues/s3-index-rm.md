---
status: unshaped
kind: feature
appetite: big
---

# `xdu-rm` against an index in S3

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu` can build an index into an S3 bucket, and `xdu-find` and `xdu-view` query it in place
(`spec/s3-index-target/`). `xdu-rm` cannot. Given an `s3://` locator, through `-i` or `XDU_INDEX`, it
refuses right after argument parsing, before any query or deletion, with `xdu-rm: S3 indices are not
yet supported by xdu-rm`, and exits non-zero (`src/bin/xdu-rm.rs`; `tests/rm_tests.rs` covers both
routes and asserts nothing else ran). An operator who keeps one central index therefore needs a
local copy of it to run a guarded bulk delete, which defeats the point of the central copy.

The deletions themselves stay local: `xdu-rm` unlinks files on the host's filesystem, and the index
only selects them. What changes is the read side, and `xdu-rm` reads more than the other readers.
Its destructive gates (`--safe` re-stat, the confirmation, `--dry-run`, the deterministic order a
`--limit` needs, the version gate and the completion warning) all sit on that read.

## Why it was deferred

A GOAL non-goal of `s3-index-target`, which was a pilot: one write path and two read-only readers.
Applying the destructive gates over a remote index is high-blast work on the one tool that deletes
files, and the pilot's appetite had no room to review it properly. The refusal (that GOAL's R8)
landed instead, so the gap fails loud. Pre-existing in the sense that `xdu-rm` never read anything
but a local index; the refusal is new.

## Outcome / vision

`xdu-rm -i s3://bucket/prefix …` selects and deletes exactly what it would select and delete from a
local copy of the same index, with every gate unchanged, and reads the index the way `xdu-find`
does: in place, by ranged reads, with the locator and partition names bound.

## Sketch of the acceptance criteria

- **R1** — WHEN `xdu-rm` is given an `s3://` index locator, it SHALL select the same files, in the
  same order, as for a local copy of that index, across its filters, `--limit` and partition scoping.
- **R2** — Every destructive gate (`--safe`, confirmation, `--dry-run`, version gate, completion
  warning) SHALL behave as it does for a local index, and none SHALL run on rows read before the
  version gate passes.
- **R3** — The S3 read SHALL meet the readers' contract: ranged reads only, no local copy, the
  locator and partition names bound, credentials only through the bound `CREATE SECRET`.
- **R4** — IF the index cannot be read (endpoint, credentials, bucket, prefix, marker), THEN
  `xdu-rm` SHALL exit non-zero having deleted nothing.

## Notes

- Related: `spec/s3-index-target/` (GOAL non-goals, R8; the reader flow in PLAN §2.6 and the
  loopback test endpoint `tests/common/s3.rs` this would reuse);
  [`local-reader-bound-parameters.md`](local-reader-bound-parameters.md), since `xdu-rm`'s local SQL
  still interpolates the partition and index path.
- Found by: `s3-index-target` (GOAL, pilot split).
