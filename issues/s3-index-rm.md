---
status: unshaped
kind: feature
appetite: big
---

# `xdu-rm`, `xdu-cp` and `xdu-mv` against an index in S3

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu` can build an index into an S3 bucket, and `xdu-find` and `xdu-view` query it in place
(`spec/s3-index-target/`). `xdu-rm` cannot. Given an `s3://` locator, through `-i` or `XDU_INDEX`, it
refuses right after argument parsing, before any query or deletion, with `xdu-rm: S3 indices are not
yet supported by xdu-rm`, and exits non-zero (`src/bin/xdu-rm.rs`; `tests/rm_tests.rs` covers both
routes and asserts nothing else ran). An operator who keeps one central index therefore needs a
local copy of it to run a guarded bulk delete, which defeats the point of the central copy.

`xdu-cp` and `xdu-mv` (#37) refuse an `s3://` index too, through `select::refuse_s3`
(`src/select.rs`), before the index, the extension directory, the destination or any query is
touched (`tests/cp_tests.rs`, `tests/mv_tests.rs`). Their cycle kept that refusal so the select-act
engine has one S3 posture, and folded the reader-side wiring it would need (a bound S3 glob, the
marker read over S3, the credential resolver) into this seed
(`spec/bulk-copy-move-xdu-cp-mv/GOAL.md`, Non-goals and Clarifications). Their copies and moves stay
local as well.

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

`xdu-cp` and `xdu-mv` joined it as a GOAL non-goal of `bulk-copy-move-xdu-cp-mv`, which was local
only to fit its calendar. Pre-existing for them in the same sense: both were born refusing.

## Outcome / vision

`xdu-rm -i s3://bucket/prefix …` selects and deletes exactly what it would select and delete from a
local copy of the same index, with every gate unchanged, and reads the index the way `xdu-find`
does: in place, by ranged reads, with the locator and partition names bound. `xdu-cp` and `xdu-mv`
do the same for a copy or a move.

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
- **R5** — R1 to R4 SHALL hold for `xdu-cp` and `xdu-mv` as for `xdu-rm`, with "copied" or "moved"
  for "deleted", through the select-act engine's one S3 read path rather than a copy per tool.

## Notes

- Related: `spec/s3-index-target/` (GOAL non-goals, R8; the reader flow in PLAN §2.6 and the
  loopback test endpoint `tests/common/s3.rs` this would reuse);
  [`local-reader-bound-parameters.md`](local-reader-bound-parameters.md), since `xdu-rm`'s local SQL
  still interpolates the partition and index path; `spec/bulk-copy-move-xdu-cp-mv/` (GOAL
  non-goals and Clarifications, the reader-side wiring);
  [`xdu-rm-on-shared-engine.md`](xdu-rm-on-shared-engine.md), which would put `xdu-rm` on that same
  engine and so give all three tools one S3 read.
- Found by: `s3-index-target` (GOAL, pilot split). Widened to `xdu-cp` and `xdu-mv` when the seed of
  `bulk-copy-move-xdu-cp-mv` retired, 2026-11-09, as that GOAL's non-goals record.
