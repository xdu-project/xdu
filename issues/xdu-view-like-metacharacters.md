---
status: unshaped
kind: fix
appetite: small
---

# `xdu-view` lists a sibling's files under a directory whose name holds `_` or `%`

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu-view` selects the rows under a directory with `path LIKE '<dir>/%'`, built in
`DirectoryQuery::new` (`src/view_query.rs`). The local arm writes `<dir>/` into the literal and the
S3 arm binds `<dir>/%` as the pattern, and neither escapes the directory. In a `LIKE` pattern `_`
matches any one character and `%` any run of them, so a directory whose name holds either
metacharacter also selects the rows of every sibling the pattern happens to match. The listing
then cuts those rows at the same offset and reports their children as if they were its own.

**Reproduction** (2026-11-06, the engine `duckdb` 1.4.4 bundles, a throwaway probe calling the
public `xdu::view_query` API against a local index that `xdu` built). The partition `p` holds
`a_b/f1`, `axb/g1`, `a%b/h1` and `aZZb/g2`:

```
"/r/p/axb/g1"  LIKE "/r/p/a_b/%" = true
"/r/p/aZZb/g2" LIKE "/r/p/a%b/%" = true
list <root>/p/a_b: ["f1", "g1", "h1"]
tree <root>/p/a_b: ["f1", "g1", "h1"]
list <root>/p/a%b: ["f1", "g1", "h1"]
tree <root>/p/a%b: ["f1", "g1", "h1"]
```

`a_b` lists `g1` (from `axb`) and `h1` (from `a%b`) beside its own `f1`, and their sizes and
counts are summed in. `a%b` lists both siblings' files too. A matched sibling whose name is longer
than the pattern's span, as `aZZb` is for `a%b`, cuts to an empty component and is dropped, so the
damage depends on name lengths. Selecting a phantom entry such as `g1` under `a_b` builds the path
`<root>/p/a_b/g1`, which no row has: the entry looks like a file of `a_b` but is not one.

## Why it was deferred

Found while planning `xdu-view-prefix-bytes-vs-chars`, whose GOAL lists it as a non-goal and whose
foreman ruling sends it here if it reproduces. That fix changes only the offset the cut uses; it
does not touch the pattern, so it neither closes nor widens this. **Pre-existing**: the unescaped
`LIKE` dates from the views' first commits and is present on `main` at `7689a92`.

## Outcome / vision

A listing of a directory reports only the rows under that directory, whatever characters its name
holds.

## Sketch of the acceptance criteria

- **R1** — WHEN `xdu-view` lists a directory whose name holds `_` or `%`, both views SHALL report
  only the entries stored under that directory, with their own sizes, counts and access times.
- **R2** — R1 SHALL hold for a local index and for an `s3://` index.
- **R3** — The fix SHALL NOT widen invariant §5: no value `main` binds is written into SQL text, and
  no directory name reaches a second literal beside the `LIKE`.

## Notes

- Candidate fixes: escape `_`, `%` and the escape character itself in the prefix and add an
  `ESCAPE` clause (the local literal and the S3 bound pattern both); or match with a prefix test
  that has no metacharacters, such as `starts_with(path, ?)`, which for a local index means binding
  the prefix. Settle which escape character DuckDB 1.4.4's `LIKE` honours by execution.
- Binding the local prefix is the territory of
  [`local-reader-bound-parameters.md`](local-reader-bound-parameters.md), which already rewrites
  that literal; this could ride with it. The `'`-in-a-directory-name failure is that seed's, not
  this one's.
- No other reader uses `LIKE` on a path: `xdu-find` and `xdu-rm` filter through `QueryFilters`.
- Related: [`xdu-view-prefix-bytes-vs-chars.md`](xdu-view-prefix-bytes-vs-chars.md) (the other
  defect in the same statement).
- Found by: `xdu-view-prefix-bytes-vs-chars` `/xdu-plan`, 2026-11-06.
