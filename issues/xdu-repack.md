---
status: unshaped
kind: feature
appetite: big
---

# `xdu-repack`: rewrite an index into a different layout

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

An index's layout is fixed at crawl time, and the only way to change it is to crawl again. On the
filesystems this tool exists for that is the expensive operation: a billion-file walk costs hours of
metadata-server load, which is the scarce resource on shared storage, not the bytes. So an
administrator who wants to try a different partitioning, move an index from local disk to object
storage, or compact a tree of thin chunks has one instrument, and it is the blunt one.

The same gap makes every index-format change a flag day. There is no way to carry an existing index
forward, so a format bump obsoletes every index on every host at once and the only remedy is to
re-crawl all of them.

Rewriting needs none of that. The index already holds every value a new layout could sort or
partition on, so a repack reads Parquet and writes Parquet and never touches the filesystem it
describes.

## Why it was deferred

Not deferred from a factory pass. Identified 2026-09-15 while shaping the layout work, as the piece
that makes the rest of it safe to adopt: a layout you can change afterwards is a decision an
operator can revise, and one you cannot is a decision they have to get right before their first
crawl.

It also absorbs the part of partitioning the crawler cannot afford. A record's partition is unknown
until it is stat'd, which is why
[`opt-in-deeper-partitioning.md`](opt-in-deeper-partitioning.md) restricts the crawler to keys
derivable from the path. A repack reads complete data, so metadata-derived keys — owner, age band,
size band — cost it nothing extra.

## Outcome / vision

Point at an index, name a destination, declare the layout with the same flags the crawler takes, and
get a second index that answers the same questions with a different cost profile. No walk, no
`stat`, no metadata-server load, and the original untouched until the new one is complete.

Measured 2026-09-15: DuckDB does the substantive work in one statement.
`COPY (SELECT … FROM read_parquet(<old glob>, filename=true)) TO <new> (FORMAT parquet,
PARTITION_BY (part, ayear))` produced `part=alice/ayear=y2021/…`, read back all 60,000 source rows,
and filtered on the derived key correctly. The work in this cycle is the safety around that
statement, not the statement.

## Sketch of the acceptance criteria

- **R1** — `xdu-repack` SHALL read an existing index and write a new one at a different location
  with layout, clustering and chunking declared by the same flags the crawler accepts, without
  stat'ing any file the index describes.
- **R2** — The written index SHALL carry exactly the documented schema — no column added, none
  removed — whatever layout was requested.
- **R3** — The new marker SHALL describe the layout actually produced, and SHALL carry forward the
  source index's crawl provenance rather than resetting it.
- **R4** — It SHALL refuse to write into its own source.
- **R5** — A failed or interrupted repack SHALL NOT leave a destination that reads as a complete
  index.
- **R6** — Either side SHALL be able to live on object storage, so a repack doubles as the
  local-to-remote publication step.

## Notes

Four traps, all observed rather than anticipated:

- **Partition keys are stripped from the files.** In the measured run, `part` and `ayear` were
  columns in the `SELECT` and are absent from the written Parquet schema — they live only in the
  path. So partitioning on a schema column would *delete that column from the index*, and
  `PARTITION_BY (uid)` silently produces a seven-column index. R2 exists for this. The fix is a
  derived alias, or the writer option that keeps partition columns in the data.
- **`SELECT *` with `filename=true` leaks a ninth column.** The probe's output carried
  `path,size,uid,gid,mode,atime,mtime,ctime,filename`. Explicit projection is not a style
  preference here; it is the only thing standing between this tool and a schema violation nothing
  would notice until a reader selected a column that is not there.
- **File naming diverges.** DuckDB writes `data_0.parquet`; the layout's zero-padded
  `NNNNNN.parquet` is what `finalize`'s stale-chunk pruning keys off. The writer's filename pattern
  is settable, so this is a thing to remember rather than a thing to solve.
- **Provenance must not be laundered.** `errors` and `vanished` describe a crawl. An index rewritten
  from one built under `--allow-errors` still knows nothing about the files that crawl skipped, and
  `xdu-rm`'s risk is exactly the files an index does not know about. Writing `errors=0` over that
  history deletes the warning the operator running `xdu-rm` weeks later depends on. R3, and the key
  it needs, come from [`self-describing-index-marker.md`](self-describing-index-marker.md).

And three shaping questions for promotion:

- **Scale is the open risk.** Partitioned writes buffer per partition, so open file handles and
  memory are where a billion rows will break this. Whether the answer is batching by source
  partition, bounded fan-out, or a spill directory is a design question, not a detail.
- **A new binary fits the pattern** — one verb per binary, as `xdu-find`, `xdu-view` and `xdu-rm`
  already are, and a natural sibling for the queued `xdu-cp`/`xdu-mv`/`xdu-tar` theme. It brings the
  usual obligations: a `doc/*.scd` page, completions from `src/cli.rs`, and a place in the tarball
  layout `install.sh` mirrors.
- **R5 has a mechanism already.** The completion marker is the commit boundary for a crawl for
  exactly this reason — written only on the success path, so an interrupted run leaves something
  every reader refuses. A repack should reuse that discipline rather than invent one.
- Related: [`opt-in-deeper-partitioning.md`](opt-in-deeper-partitioning.md) (whose metadata-derived
  half this absorbs), [`self-describing-index-marker.md`](self-describing-index-marker.md)
  (prerequisite), [`chunk-clustering-and-row-groups.md`](chunk-clustering-and-row-groups.md) (a
  repack re-clusters without re-crawling), `spec/s3-index-target/` (on `main` since #33; the
  `s3://` locator, writer and readers R6 builds on),
  [`index-diff-incremental-select.md`](index-diff-incremental-select.md) (the other tool that reads
  one index and writes another).
- Found by: the DuckDB-native S3 read spike and the layout design that followed it, 2026-09-15.
