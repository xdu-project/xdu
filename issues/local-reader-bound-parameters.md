---
status: unshaped
kind: fix
appetite: big
---

# Bind the index path and partition names in local-index reader SQL

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

Invariant §5 (`.agents/factory/invariants.md`) records a known exception: for a **local** index, the
readers interpolate the index path and partition names into SQL text raw. `lib::parquet_source`
returns `read_parquet('<index>/<partition>/*.parquet')` with the glob written into a string literal,
and every statement of `xdu-find`, `xdu-view` and `xdu-rm` takes its `FROM` from there or from
`lib::index_glob` directly. For an `s3://` index the same function returns `read_parquet(?)` and
binds the glob, so the S3 side is already closed.

The raw text has observable effects:

- A `'` in the index path or a partition name ends the literal. The query fails with a parser error
  instead of answering, and in `xdu-rm`, the tool that deletes, the text after the quote is SQL.
- `xdu-view` also writes a data-derived value into text: the directory a listing is under, as
  `path LIKE '<dir>/%'`. A directory whose name holds a `'` cannot be listed from a local index. Over
  S3 the same listing works, because the S3 arm binds that prefix
  (`view_query::DirectoryQuery`; the wire test
  `view_quotes_in_the_prefix_and_a_directory_match_local` lists such a directory over S3 only, for
  this reason).
- Glob metacharacters in a local index path are read as a glob.

A related cleanup rides along. `xdu-view`'s list and tree views send the same directory statement in
two whitespace layouts (`list_directory_sql`, `tree_directory_sql` in `src/view_query.rs`). They were
kept apart only so that local SQL stayed byte-for-byte unchanged; once local SQL is rewritten anyway,
one builder serves both.

## Why it was deferred

A GOAL non-goal of `s3-index-target`, ruled at that cycle's GOAL gate as a pre-emptive cut: binding
local SQL rewrites every reader statement on a high-blast path, `xdu-rm` included, for a gain the S3
pilot did not need. That cycle kept local SQL byte-identical and pinned it with unit tests
(`test_parquet_source_keeps_local_sql_byte_for_byte`,
`view_query::tests::local_view_sql_is_byte_for_byte_what_xdu_view_sent`). Pre-existing: the raw
interpolation predates S3 support.

## Outcome / vision

No reader puts an index path, a partition name or a directory prefix into SQL text, for any index.
Invariant §5's local exception is retired, and a quote in any of them is just a byte.

## Sketch of the acceptance criteria

- **R1** — WHEN any reader queries a local index, the index path, any partition name and any
  directory prefix that reaches SQL SHALL be a bound parameter.
- **R2** — A local index whose path, partition name or directory name contains `'` or a glob
  metacharacter SHALL answer exactly as the same index without it does.
- **R3** — `xdu-rm` SHALL select the same files as before for every index that worked before (a
  parity test over the existing fixtures), since it deletes what it selects.

## Notes

- Related: invariant §5; `spec/s3-index-target/` (GOAL non-goals and the R6 clarification; P4's
  run A amendment on the two directory layouts);
  [`s3-index-rm.md`](s3-index-rm.md), which touches the same `xdu-rm` statements.
- Found by: `s3-index-target` (GOAL gate), with the `xdu-view` findings from its P4.
