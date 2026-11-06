---
status: unshaped
kind: fix
appetite: small
---

# A debug build aborts on a local filter-and-sort query inside the bundled DuckDB

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

A **debug** build of `xdu-find` aborts on one local query shape. Over an index built from the S3
parity fixture (`varied_source_tree` in `tests/common/s3.rs`: 1400 files, every size distinct), the
query

```
xdu-find -i <local index> -u alice --min-size 500 -f size
```

stops the process with a failed assertion inside the bundled engine:
`Assertion failed: (min_val <= input)`, in `compress_integral.cpp`. The **release** build answers the
same query correctly. The SQL is the local reader's existing text, byte for byte, so the abort is in
the engine's debug-only checks, reached by this combination of partition scope, size filter and
size sort. It is not in xdu's SQL.

It matters because `cargo test` runs debug binaries. A test over this shape aborts the test binary
instead of failing an assertion, and a developer chasing a real bug in that area hits the abort
first.

## Why it was deferred

Found in `s3-index-target` P2, whose filter-parity test now avoids that exact combination and keeps
its other size and sort cases (TECH P2 run A amendment; foreman ruling at P2 run B). Fixing it means
reporting it upstream or moving the engine, and bumping `duckdb` was a non-goal of that cycle.
Pre-existing: the query and the engine both predate that cycle.

## Outcome / vision

The query shape runs in debug builds as it does in release builds, or xdu documents why it cannot,
and the parity test covers the combination again.

## Sketch of the acceptance criteria

- **R1** — WHEN a debug build runs the reproduction above over the parity fixture, it SHALL answer
  with the release build's output.
- **R2** — The S3 filter-parity test SHALL include that combination again once R1 holds.
- **R3** — IF the cause is upstream and unfixed in the pinned engine, THEN a minimal reproduction
  SHALL be reported upstream and linked here.

## Notes

- Related: `spec/s3-index-target/` (TECH P2's run A amendments, "An engine assertion avoided, not
  fixed"); [`macos-sdk27-bundled-duckdb-build.md`](macos-sdk27-bundled-duckdb-build.md), the other
  open item on the bundled engine; an engine bump also re-pins the shipped extensions.
- Found by: `s3-index-target` P2.
