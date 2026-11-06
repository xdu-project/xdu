---
status: unshaped
kind: fix
appetite: small
---

# `xdu-view` truncates child names under a directory whose path is not ASCII

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu-view` lists a directory by matching `path LIKE '<dir>/%'` and cutting the next path component
out of each row in SQL. `DirectoryQuery::new` in `src/view_query.rs` measures the prefix in Rust:

```
let prefix = format!("{}/", dir);
let prefix_len = prefix.len();
```

`str::len` is a byte count. Both `list_directory_sql` and `tree_directory_sql` then splice that
number into `substr(path, {prefix_len} + 1)`, and DuckDB's `substr` counts characters. Every
non-ASCII character anywhere in `<dir>` is two or more bytes but one character, so the cut lands
that many characters too far right and each child name loses its leading characters.

The prefix is the whole absolute path, not the partition-relative part, so the trigger is a
non-ASCII character anywhere above the listed directory: a partition named `données`, a directory
below it, or the crawled root itself (a home directory under an accented user name breaks every
listing).

**Reproduction** (2026-11-06, `main` at `d6b82eb`, a scratch integration test calling the public
`xdu::view_query` API against a local index that `xdu` built). The tree is
`<tmp>/tree/{data,données}/{alpha,beta}/file` plus a `top` file in each partition, so the partition
root is the partition directory itself:

```
top="data" root_ends="data"
  list_directory names = ["alpha", "beta", "top"]
  tree_directory names = ["alpha", "beta", "top"]
top="données" root_ends="données"
  list_directory names = ["eta", "lpha", "op"]
  tree_directory names = ["eta", "lpha", "op"]
```

One `é` costs one leading character. `directory_entries` then builds each entry's `path` as
`format!("{dir}/{component}")`, so selecting `lpha` descends into `<root>/données/lpha`, which
matches no rows: the subtree is unreachable in both views, not merely mislabelled. A name shorter
than the overshoot becomes empty and is dropped by `component != ''`, so it vanishes from the
listing while its size still counts nowhere.

## Why it was deferred

**Pre-existing**, not introduced by `s3-index-target`: the same `prefix.len()` and `substr` text
date from the filter/sort and Miller-columns commits that first built these views, and the
module's unit test pins the local statement byte for byte against the old text. Found while
retiring that cycle's seed, outside any cycle, so it is filed rather than fixed on a retirement
branch.

## Outcome / vision

Both views list the same child names under a non-ASCII directory as under an ASCII one, and every
listed directory can be entered.

## Sketch of the acceptance criteria

- **R1** — WHEN `xdu-view` lists a directory whose absolute path contains non-ASCII characters, in
  the list view or the tree view, it SHALL report each child under its full name, identical to the
  name on disk.
- **R2** — WHEN the operator enters a directory so listed, the view SHALL list that directory's own
  children.
- **R3** — A test SHALL index a tree with a non-ASCII directory above the listed one, for a local
  index and for an `s3://` index where the bucket layout admits it, and assert both views' names.

## Notes

- The S3 arm shares `DirectoryQuery` and the same `substr` text, so by construction it carries the
  same defect for any non-ASCII directory below the partition (reasoned, not executed: the
  reproduction above is local only). Only the partition name is kept ASCII there, by the writer's
  refusal ([`s3-non-ascii-partition-names.md`](s3-non-ascii-partition-names.md)); deeper names are
  row data, not key segments.
- Candidate fixes: a character count (`prefix.chars().count()`), or computing the offset in SQL from
  the prefix itself so the two can never disagree. Confirm at promotion that DuckDB's `substr` and
  `length` count the same unit (code points, not grapheme clusters) before relying on either.
- The local-statement unit test `local_view_sql_is_byte_for_byte_what_xdu_view_sent` pins the
  current text, so the fix changes that pin deliberately.
- `xdu-view.rs` slices `current_path[partition_root.len()..]` in Rust for display; both sides are
  Rust byte lengths there, so that site is consistent and not part of this defect.
- Related: [`xdu-view-terminal-safety.md`](xdu-view-terminal-safety.md) (the other multibyte defect
  in the same views), [`local-reader-bound-parameters.md`](local-reader-bound-parameters.md) (binds
  the same local prefix, and could carry this fix).
- Found by: the `s3-index-target` retirement sweep, 2026-11-06.
