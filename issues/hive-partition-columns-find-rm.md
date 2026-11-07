---
status: unshaped
kind: fix
appetite: small
---

# `xdu-find` and `xdu-rm` read a `key=value` directory name as a column

> **Pre-shaped candidate, not a contract.** This file records deferred work in enough detail that a
> future session does not have to re-derive it. It is **not** graded by `xdu-review` and must never be
> copied into `spec/{slug}/GOAL.md` verbatim — `/xdu-feature` promotes it, and that is where appetite,
> non-goals and the R-IDs get negotiated with a human. The `status:` field above is the guard, with
> three states: `unshaped` means proposal; `shaped on <branch> (<date>) — see spec/<slug>/GOAL.md`
> means a cycle adopted it and is still in flight; `resolved on <branch> (<date>) — see
> spec/<slug>/` means the cycle landed on `main`. Only `/xdu-feature` writes `shaped`, only the
> landing cycle writes `resolved`, and `/xdu-roadmap` deletes the file once the landing is confirmed.
>
> Deliberately **not** named `GOAL-{slug}.md`: every other GOAL in the factory is a locked contract, so
> a file carrying that name eventually gets treated as one.

**Priority: high.** In `xdu-rm` this selects the wrong files to delete.

## Problem

DuckDB's `read_parquet` auto-detects Hive partitioning: when every path it globs carries a
directory named `key=value`, it adds a column `key` holding `value`, and that column shadows a
column of the same name in the file. The index layout uses bare directory names, but nothing stops
a top-level source directory from being named `size=0`, or an index from being stored under a
directory such as `…/mtime=0/`. Then every row read through that glob has `size` 0, or `mtime` 0,
whatever the crawler recorded.

`xdu-find` (through `lib::parquet_source`) and `xdu-rm` (its own `read_parquet('{glob}')` in
`src/bin/xdu-rm.rs`) both leave the detection on. Reproduced on
`feature/bulk-copy-move-xdu-cp-mv` (review cycle 1, finding 3, probe `hive2.sh`): with a 5,000,000
byte file under the partition `size=0`, `xdu-find -u size=0 --max-size 1K` lists it, and
`xdu-rm -u size=0 --max-size 1K -n` previews deleting it. An index under a `mtime=0` directory makes
`--mtime-older-than 3650` match files written seconds earlier. Without `-u`, a mixed index usually
reads correctly, because detection needs every globbed path to carry the key; an index path that
carries one breaks every query.

`xdu-rm` is destructive: it deletes the rows the shadowed column selects, so a size or time filter
meant to protect files does not. `xdu-rm --safe` re-checks only atime and maximum size, so it
catches the `--max-size` case and not the `--mtime-*` ones.

`src/view_query.rs` builds its statements through the same `parquet_source` and should be
checked in the same pass; its comment at `partitions_sql` says Hive partitioning "would yield no
partition column at all", which is true of the layout but not of a `key=value` name.

## Why it was deferred

Found by the blind review of `bulk-copy-move-xdu-cp-mv`, whose engine (`src/select.rs`) now reads
with `hive_partitioning = false`. `xdu-find`, `xdu-rm` and `view_query.rs` were out of that cycle's
scope (`xdu-rm` and its tests are a GOAL non-goal there, and `parquet_source` was untouchable).
**Pre-existing**: present on `main`.

## Outcome / vision

Every reader of a local or S3 index reads the columns the crawler wrote and nothing else. A
`key=value` partition name or index path selects the same rows as any other name.

## Sketch of the acceptance criteria

- **R1** — WHEN `xdu-find`, `xdu-rm` or `xdu-view` reads an index, the reader SHALL pass
  `hive_partitioning = false` (or an equivalent) to every `read_parquet` call, so no directory
  name becomes a column.
- **R2** — WHEN a partition is named `size=0`, THEN `xdu-find -u size=0 --max-size 1K` and
  `xdu-rm -u size=0 --max-size 1K -n` SHALL select only files whose indexed size is at most 1K.
- **R3** — WHEN the index is stored under a `mtime=0` directory, THEN `--mtime-older-than` SHALL
  compare the indexed mtime, for each reader.
- **R4** — `AGENTS.md`'s project paragraph SHALL name every reader as covered once R1 lands.

## Notes

- Related: `issues/xdu-rm-on-shared-engine.md` (moving `xdu-rm` onto the engine would carry the
  fix for its share); `issues/local-reader-bound-parameters.md` (rewrites the same local reader
  statements); `spec/bulk-copy-move-xdu-cp-mv/REVIEW.md` finding 3.
- Found by: `bulk-copy-move-xdu-cp-mv` review cycle 1, filed in its remediation.
