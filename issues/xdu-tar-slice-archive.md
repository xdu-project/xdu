---
status: unshaped
kind: feature
appetite: big
---

# Slice archives: `xdu-tar`

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

There is no way to archive a slice of a tree — "all files older than X", "everything changed since
the last backup" — with the tree structure preserved. `xdu-find | xargs tar` inherits every `xargs`
failure (chunking at `ARG_MAX`, no dry-run, no confirmation, no re-stat, unstable order across
invocations) and adds a tape-hostile one: an archive assembled from ten thousand appends instead of
one ordered stream. The motive is incremental backup to tape from million-file trees, where a
partial set meeting a criterion must become one `.tar` (or compressed equivalent) whose layout
restores onto the original paths.

## Why it was deferred

Not deferred from a factory pass. Split from `issues/xdu-mv-and-xdu-tar.md` with the copy/move
seed, and sequenced second in the theme so it builds on the shared select-act engine and `cp`'s
staging semantics. Full incremental selection waits on the diff seed, third; time-filtered slices
ship first.

## Outcome / vision

`xdu-tar` archives exactly the matched set with the tree structure preserved, as one ordered stream
suitable for file or tape — a full slice today, an incremental slice once diffing supplies the
selection.

## Sketch of the acceptance criteria

- **R1** — The archive SHALL preserve the tree: relative paths under a documented root, parent
  directories as explicit entries; prefix and strip handling are decided at shaping.
- **R2** — The index holds only regular files, so empty directories, symlinks, and mode/ownership
  bits are not in the rows. The cycle SHALL either read them from the live tree at archive time or
  document the omission in the man page; silent loss is not an option.
- **R3** — Staging the slice via `cp` semantics to a prefix and shelling out to `tar`, versus
  streaming the archive directly from the matched set, SHALL be decided at planning — weighed on
  scratch cost (staging needs disk for the whole slice and doubles I/O) against tape behavior
  (streaming never touches disk).
- **R4** — The archive SHALL be writable to stdout, so it can be piped to a compressor or a tape
  device without an intermediate file; compression (none/gzip/zstd) is decided at shaping.
- **R5** — The same selection SHALL yield the same member order on every run (deterministic
  `ORDER BY`), so archives are comparable and downstream tooling can reason about them.
- **R6** — Dry-run listing, confirmation, `--safe` re-stat, the completion-marker warning, and
  `--limit` determinism come from the shared engine; files vanishing between selection and
  archiving are counted, not fatal — the `rm` missing-count behavior.

## Notes

- Second of the bulk-operations theme, after `xdu-cp` and `xdu-mv`, whose select-act engine is on
  `main` since #37 (record in `spec/bulk-copy-move-xdu-cp-mv/`); incremental selection arrives with
  `issues/index-diff-incremental-select.md`.
- Related: GitHub issue #1.
- Found by: backup-vision scoping 2026-09-07; split from `xdu-mv-and-xdu-tar.md`.
