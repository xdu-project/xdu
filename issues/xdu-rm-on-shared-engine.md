---
status: unshaped
kind: refactor
appetite: small
---

# `xdu-rm` on the shared select-act engine

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`bulk-copy-move-xdu-cp-mv` built a library select-then-act engine for `xdu-cp` and `xdu-mv`, on
`main` since #37: the gates, a bound-parameter selection, deterministic `ORDER BY path` under
`--limit`, dry-run and confirmation, a `--safe` re-stat that `lstat`s and re-checks every filter
given, and counted outcomes. `xdu-rm`, whose `main` the pattern came from, stays on its own copy,
with the gaps the engine closes. Confirmed against `7689a92`; line numbers re-read at `5d551a0`:

- `--safe` re-checks only atime (`--older-than`) and size (`--max-size`)
  (`src/bin/xdu-rm.rs:161-193`); `--min-size` and `--newer-than` are unchecked, as invariant §4
  records. The man page claims more (`doc/xdu-rm.1.scd:64-67`), and its own example
  `xdu-rm --min-size 1G --safe` (`:105`) re-checks nothing. The re-stat is `fs::metadata`, which
  follows a symlink planted at the path.
- The selection writes the local glob, index path and partition name into a SQL string literal
  (`src/bin/xdu-rm.rs:88`), §5's raw-interpolation exception in the one tool that deletes: a `'`
  in either fails the query, and the text after it is SQL.
- `xdu-rm` exits 0 after failed unlinks (`:246`).
- Its flag set stops at pattern, partition, size and atime (`src/cli.rs:210-243`), while
  `xdu-find`, `xdu-cp` and `xdu-mv` also filter by owner, group, mode and mtime.

## Why it was deferred

Ruled out of `bulk-copy-move-xdu-cp-mv` at its GOAL gate (2026-11-06), to keep high-blast
`src/bin/xdu-rm.rs` off a calendar-bound critical path and leave `xdu-rm`'s behavior stable for
the release users have. Pre-existing on `main`: every gap above predates that cycle.

## Outcome / vision

`xdu-rm` runs on the same engine as `xdu-cp` and `xdu-mv`, so its `--safe` checks what its man page
says, its selection binds the index glob, and a fix to the engine lands in every select-act tool at
once.

## Sketch of the acceptance criteria

- **R1** — `xdu-rm` SHALL select and act through the shared engine, with `tests/rm_tests.rs`
  passing; any assertion that changes is a stated behavior change, not a silent one.
- **R2** — WHEN `--safe` is given, `xdu-rm` SHALL `lstat` each file immediately before unlinking
  it and skip it unless it is still a regular file and every filter given still holds, closing §4's
  gaps without weakening the atime and size checks; the man page SHALL describe what is checked.
- **R3** — The selection SHALL bind the index glob, so a `'` in the index path or a partition name
  selects the right rows.
- **R4** — Shaping decides whether `xdu-rm` exits non-zero on failed unlinks, as `xdu-cp` and
  `xdu-mv` do, and whether it gains `xdu-find`'s owner, group, mode and mtime filters.

## Notes

- Its R3 retires `xdu-rm`'s share of `issues/local-reader-bound-parameters.md`, which then covers
  `xdu-find` and `xdu-view`.
- An `s3://` index for the engine stays with `issues/s3-index-rm.md`.
- The migration also adds `*xdu-cp*(1)` and `*xdu-mv*(1)` to the SEE ALSO of `doc/xdu-rm.1.scd`.
  The copy/move cycle cross-referenced every other page but left `xdu-rm.1` untouched, under its
  non-goal of no change to `xdu-rm`; re-count that page's asserted literals in the same commit.
- Related: `spec/bulk-copy-move-xdu-cp-mv/GOAL.md` (Non-goals, Clarifications); invariant §4.
- Found by: `bulk-copy-move-xdu-cp-mv` GOAL gate, 2026-11-06.
