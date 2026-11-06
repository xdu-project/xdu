---
status: unshaped
kind: feature
appetite: big
---

# Opt-in deeper partitioning, declared at crawl time

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

The index has exactly one partition level and it is fixed: the top-level subdirectory. An
administrator whose tree is `/scratch/<user>/<project>` cannot partition per project; one who mostly
asks age questions cannot partition by age at all. `-u/--partition` takes a single NAME, so there is
no way to ask about several partitions in one query either.

The level is also the unit of scoped re-indexing. `-p` re-crawls whole top-level directories, so a
partition is simultaneously the thing a query prunes to and the thing a re-crawl rewrites, and at
depth 1 both are coarser than an operator on a shared filesystem wants.

What the layout is **not** is a defect. Measured 2026-09-15 against a 120-prefix index in S3:

| selecting one partition | LIST | GET | MiB | ms |
|---|---|---|---|---|
| glob-scoped to the prefix (today) | 2 | **1** | 0.02 | 14 |
| `hive_partitioning=true` + `WHERE partition='p42'` | 2 | 2 | 0.09 | 41 |

Today's mechanism wins that case, because narrowing the glob narrows the LIST prefix while a
partition predicate only filters the file list after listing. Hive naming prunes file opens; it
never prunes listing. So this is not a migration to something better — it is an opt-in to expressing
predicates the glob cannot, where the gain is real: `WHERE ayear='2022'` across all 120 prefixes
read 25 files instead of 120, and `WHERE partition IN (…)` over three of them read 4 instead of 120.

## Why it was deferred

Not deferred from a factory pass. Reshaped 2026-09-15 from an earlier seed that framed this as a
Hive-partitioning *migration*, which the measurement above shows is the wrong question — the default
should not move.

The prose half of that earlier seed is already resolved: `README.md`, `AGENTS.md`, `ROADMAP.md`,
`doc/xdu.1.scd` and the stale comment in `src/bin/xdu-view.rs` no longer call the layout
Hive-partitioned, since bare directory names yield no partition column and `hive_partitioning=true`
over them derives nothing. What is left is the capability question, which is this seed.

## Outcome / vision

The simple case stays the default and stays fastest. An administrator who knows what their site asks
declares one more level at crawl time, the index records that choice, and every tool reads it
correctly without being told. An index built for a `/scratch/<user>/<project>` tree prunes to a
project; one built where age is the question prunes to an age band; one built by an operator who
declared nothing behaves exactly as it does today.

Crucially, the crawler only ever takes on keys it can derive from the path, which are known before
any `stat` and therefore cost nothing. Keys that need file metadata are a batch problem, and
[`xdu-repack.md`](xdu-repack.md) is where they belong.

## Sketch of the acceptance criteria

- **R1** — The default layout SHALL be unchanged: one level, bare names, glob-scoped pruning, and no
  marker key required to read it.
- **R2** — `xdu` SHALL accept an opt-in declaration of one further partition level at crawl time,
  derived from the path so that a record's destination is known before it is stat'd.
- **R3** — The declared layout SHALL be recorded in the marker such that a reader constructs the
  correct read from it rather than from the directory tree.
- **R4** — A reader SHALL keep glob-scoped pruning when the declared value is known, and gain
  predicate pruning over that level when it is not.
- **R5** — The crawler's single-writer-per-partition model SHALL be preserved: one driver owns a
  top-level prefix and everything beneath it, so chunk-id allocation and stale-chunk pruning keep
  working unchanged.
- **R6** — The tool SHALL refuse, or warn loudly, on a declaration that would produce partitions too
  thin to write efficiently.

## Notes

- **One layout, not two.** An earlier sketch branched on key type — bare directories for
  path-derived keys, `key=value` for others. Unnecessary: the two compose. The measured 1-GET glob
  case above was itself taken against a `partition=p42/` prefix, so hive naming keeps glob pruning
  *and* adds predicate pruning. Two shapes would mean two reader code paths for one capability,
  which is where the bugs will be. The live trade is legibility — `alice/project1/` is what an
  operator expects from `ls`, and for a path-derived level the key name is arbitrary anyway — not
  capability.
- **`=` in a directory name is the new reserved-name class.** Hive naming makes a source directory
  called `foo=bar` ambiguous at the point a reader parses the path. `RESERVED_INDEX_NAMES` already
  guards the names that collide with the layout; this widens what "collides" means, and the guard
  iterates its list precisely so a new entry rejects by construction.
- **R6's limit is file size, not listing.** LIST cost was flat at 2 calls for both 4 and 120
  prefixes, because an object store returns up to 1000 keys per call — ten thousand partitions is
  roughly ten LIST calls, not a catastrophe. The real hazard is the small-file problem: every prefix
  needs at least one Parquet file, and thin partitions collapse the row groups that
  [`chunk-clustering-and-row-groups.md`](chunk-clustering-and-row-groups.md) exists to make
  selective. Over-partitioning destroys a measured 8x to buy a smaller one.
- **Metadata-derived keys are deliberately out of scope**, and the reason is the concurrency model
  rather than the interface. A record's partition is unknown until it is stat'd, so every driver
  could produce rows for every partition: either a buffer per driver per key, or locking on the
  crawl hot path, or a shuffle stage — and single-writer-per-partition is what makes `finalize`'s
  stale-chunk pruning correct. `xdu-repack` gets the same capability for free because it reads
  complete data.
- **Sequenced after** [`self-describing-index-marker.md`](self-describing-index-marker.md) (R3 and
  R4 are unenforceable without it) **and after**
  [`chunk-clustering-and-row-groups.md`](chunk-clustering-and-row-groups.md) (whose granularity this
  trades against). Landing this first would measure partitioning against a baseline leaving 8x
  unclaimed and reach the wrong conclusion about how much partitioning is worth.
- Related: [`xdu-repack.md`](xdu-repack.md),
  [`research/duckdb-native-s3-reads.md`](../research/duckdb-native-s3-reads.md) (where the numbers
  were taken, for the S3 index target now on `main` since #33),
  [`orphan-partition-survives-reindex.md`](orphan-partition-survives-reindex.md) and
  [`index-diff-incremental-select.md`](index-diff-incremental-select.md) (both read the layout, and
  both benefit from a finer unit of scoped re-indexing),
  [`access-scoped-queries.md`](access-scoped-queries.md) (whose served-index design already rejected
  store-enforced scoping, so a partition level is a pruning key there and not a boundary).
- Found by: the DuckDB-native S3 read spike, 2026-09-15; reshaped from the earlier
  `hive-partition-layout-migration` seed the same day.
