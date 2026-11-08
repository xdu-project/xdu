---
status: shaped on feature/bulk-copy-move-xdu-cp-mv (2026-11-06) — see spec/bulk-copy-move-xdu-cp-mv/GOAL.md
kind: feature
appetite: big
---

# Bulk copy and move: `xdu-cp` and `xdu-mv`

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu-rm` proved the select-then-act pattern — an index query names exactly the set, and dry-run,
confirmation, `--safe` re-stat, and deterministic ordering under `--limit` guard the action — but
relocation still goes through `xdu-find | xargs cp|mv` plumbing with none of it. `xargs` chunking
splits the set across invocations at `ARG_MAX`, so the preview and the action can diverge without
saying so, and a stale index row moves a file that no longer matches anything. "Move everything
untouched in two years to cold storage" and "copy this user's stale logs to a staging prefix" are
single safe commands waiting to exist — and the staging copy is the primitive the archive tool
builds on.

## Why it was deferred

Not deferred from a factory pass. Split from `issues/xdu-mv-and-xdu-tar.md` during backup-vision
scoping (2026-09-07), which added `xdu-cp`, the tape-backup motive, and index diffing — more than
one cycle can shape. Sequenced first in the bulk-operations theme, after richer search and the
schema work, so both tools are born with the final filter surface and column set instead of shipping
against regex-and-`atime` and retrofitting. Pre-existing gap: no copy/move tool exists on `main`.

## Outcome / vision

`xdu-cp` and `xdu-mv` act on exactly the matched set under `xdu-rm`'s safety model, and the
select-then-act path they share with `rm` (and later `tar`) lives in one `lib` implementation
rather than a fourth copy of `rm`'s `main`.

## Sketch of the acceptance criteria

- **R1** — The shared select-act path (completion-marker warning, `WHERE` build, deterministic
  `ORDER BY` under `--limit`, dry-run/confirm/`--force`, parallel executor, summary counts) SHALL
  live in `lib` behind one implementation `xdu-rm`, `xdu-cp`, and `xdu-mv` all use.
- **R2** — WHEN files match an index query, `xdu-cp` SHALL copy exactly that set under `--into`
  with tree-preserving layout; collision/overwrite policy and any flatten option are decided at
  shaping.
- **R3** — WHEN files match an index query, `xdu-mv` SHALL relocate exactly that set under
  `rm`-level destructive safety (confirmation by default, `--force` to skip); shaping decides
  rename-vs-copy-plus-unlink and the cross-filesystem behavior.
- **R4** — WHEN `--limit` caps a copy or move, the query SHALL carry `ORDER BY path`, so dry-run
  and the real run select identical rows (invariant §4, extended from deletion to relocation).
- **R5** — `--safe` SHALL re-stat each file immediately before acting and skip files whose size,
  atime, or mtime no longer match the selection; the checked set is final at shaping, against the
  richer schema's columns.
- **R6** — Both tools SHALL expose the full filter surface richer search landed — glob alongside
  regex, at minimum whatever `xdu-find` accepts — not a subset frozen at this writing.

## Notes

- First of the bulk-operations theme; second is `issues/xdu-tar-slice-archive.md`, third is
  `issues/index-diff-incremental-select.md`.
- Sequenced after the delivered glob pilot and index format versioning (records in
  `spec/richer-search-glob-fuzzy-fulltext/` and `spec/index-schema-versioning/`), and the delivered
  richer schema (record in `spec/richer-index-schema/`).
- Related: GitHub issue #1.
- Found by: backup-vision scoping 2026-09-07; split from `xdu-mv-and-xdu-tar.md`.
