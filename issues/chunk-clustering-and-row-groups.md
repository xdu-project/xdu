---
status: unshaped
kind: feature
appetite: small
---

# Chunk clustering and row-group sizing

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

Every chunk `xdu` writes is a single Parquet row group, so statistics can eliminate whole chunk
files and nothing inside one. `PartitionBuffer::flush_chunk` builds `WriterProperties` with
compression only (`src/crawl.rs:480`); `parquet` defaults to 1,048,576 rows per row group and
`--buffsize` defaults to 100,000, so the default never reaches the boundary.

Records also arrive in walk order, which approximates path order and correlates with nothing else.
The question this tool exists to answer — what has not been touched in two years — therefore reads
every byte of every chunk, because no row group's `atime` range excludes anything.

Measured 2026-09-15 over an S3-backed index, selecting the oldest 5% by `atime`, identical data and
identical layout in all four rows:

| chunk written as | GET | MiB | vs. today |
|---|---|---|---|
| walk order, one row group (**today**) | 20 | 5.58 | — |
| sorted by `atime`, one row group | 20 | 4.79 | 1.2x |
| walk order, ten row groups | 110 | 5.66 | worse |
| sorted by `atime`, ten row groups | 20 | **0.65** | **8.6x** |

`sum(size)` over the same selection moves 10.07 MiB today and 1.04 MiB sorted with ten row groups.

The third row is the one to keep in view: row-group sizing **without** clustering is a regression,
paying five times the round trips for the same bytes because every group still has to be opened.
The two changes are one change.

## Why it was deferred

Not deferred from a factory pass. Extracted 2026-09-15 from the DuckDB-native S3 read spike, which
measured it while establishing the cost model for remote reads.

Separated deliberately, and sequenced **first**: it changes no index format, no layout and no
concurrency, and partition granularity trades directly against row-group granularity. Deciding how
records are ordered inside a chunk before deciding how chunks are split across prefixes keeps the
second decision honest — over-partitioning destroys exactly the pruning this buys, and a
partitioning cycle that lands first would be measured against a baseline that leaves 8x on the
table.

## Outcome / vision

An administrator says what the index will be asked about, and the crawler orders each chunk so the
Parquet footer can answer it. Nothing about the layout changes, no reader needs to be told anything,
and an index built for age auditing stops reading path bytes to find old files.

Clustering is self-describing: DuckDB prunes from row-group statistics with no configuration, so
this buys its speedup on local and S3-backed indices alike and on every index format, current or
future.

## Sketch of the acceptance criteria

- **R1** — `xdu` SHALL accept a declared clustering key at crawl time and order records within each
  chunk by it, with path as the default.
- **R2** — Row-group size SHALL be set so statistics eliminate row groups *within* a chunk rather
  than only whole chunks, and SHALL NOT be settable to a value that reintroduces the one-group case
  by accident.
- **R3** — The declared key SHALL be recorded in the completion marker as **descriptive** metadata:
  a reader that does not recognise it carries on, because correctness does not depend on it.
- **R4** — The crawl-time cost of the default SHALL be measured in `bench/` before it lands, on more
  than one tree shape.

## Notes

- **Neither half ships alone.** Sorting without row groups is 1.2x; row groups without sorting is
  slower than today. A phase plan that separates them will measure noise and draw the wrong
  conclusion.
- **The default is not a no-op.** Today's order is jwalk's walk order, not a sort, so choosing path
  as the default means every chunk pays a real sort on the crawl hot path. Probably noise against
  hours of `stat` calls, which is what R4 exists to establish rather than assume — and a
  nearly-sorted input may make it cheaper than a general sort suggests.
- **Path clustering helps prefix predicates only.** Min/max statistics can exclude a row group for
  `path LIKE '/scratch/alice/%'`; they can do nothing for the substring and regex forms `-p`
  accepts. Worth stating in the flag's own documentation so the default is not mistaken for a
  general speedup.
- **One key, not several.** Clustering is a total order over the chunk; a second key only refines
  ties. Whether a composite (`uid`, then `atime`) is worth the interface is a promotion question.
- **No format bump.** R3 keeps the marker key descriptive, so a current reader meets a clustered
  index and reads it correctly without knowing the concept exists. That is what makes this safe to
  land ahead of [`self-describing-index-marker.md`](self-describing-index-marker.md) rather than
  behind it.
- Related: [`self-describing-index-marker.md`](self-describing-index-marker.md) (where the key is
  recorded once the marker grows a schema),
  [`opt-in-deeper-partitioning.md`](opt-in-deeper-partitioning.md) (the granularity this trades
  against), [`xdu-repack.md`](xdu-repack.md) (which re-clusters an existing index without
  re-crawling), [`research/duckdb-native-s3-reads.md`](../research/duckdb-native-s3-reads.md)
  (where the measurement was taken, for the S3 index target now on `main` since #33).
- Found by: the DuckDB-native S3 read spike, 2026-09-15.
