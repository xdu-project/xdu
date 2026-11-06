---
status: unshaped
kind: fix
appetite: small
---

# `xdu-mv` across filesystems: close the source-name swap race with a quarantine rename

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

## Problem

A cross-filesystem `xdu-mv` (`atomic_fs::move_across` in `src/atomic_fs.rs`) copies the file,
flushes the copy and its directory, then re-checks the source name with
`fstatat(parent, name, AT_SYMLINK_NOFOLLOW)` and compares device, inode, type, size, mtime and
ctime against the `fstat` taken when the source was opened. Only then does it call
`unlinkat(parent, name, 0)`. The re-check and the unlink are two system calls. Removing the name
relative to the held parent descriptor closes swaps of the directories above it, but not a swap
of the last name inside that directory: another process that renames a different file onto the
source name between the re-check and the unlink gets its file removed in place of the one that
was copied. POSIX has no unlink-if-inode-matches. `xdu-mv(1)`, CROSSING FILESYSTEMS, lists this
race with the others the checks narrow but cannot close.

The same shape exists on the link route of `rename_noreplace` (NFS's main path), which re-checks
the source inode after `linkat` and before `unlinkat`.

## Why it was deferred

The calendar for `bulk-copy-move-xdu-cp-mv` held the cross-filesystem path itself as the first
cut, so hardening beyond the plan's sequence was out of its box. The plan recorded the quarantine
rename as a possible hardening, not built (`spec/bulk-copy-move-xdu-cp-mv/PLAN.md` §5;
`research/02-cross-filesystem-ci.md` §2). Introduced by that cycle, in the sense that the
cross-filesystem move is new there; the window is narrow (two syscalls) and needs a hostile or
very unlucky process in the source directory.

## Outcome / vision

The name `xdu-mv` removes is always the file it copied. Before the unlink, the source is renamed,
no-replace, to a private name in its own directory (`.xdu-mv-<pid>-<n>`, matching the temp naming
the man pages already tell operators to look for); that name is `fstatat`'d and its inode compared
with the opened file's; only then is the private name removed. A file swapped in at the original
name is left there under that name. A crash between the quarantine rename and the unlink leaves a
`.xdu-mv-*` file in the source directory, while the destination copy is already durable, so
nothing is lost and the leftover is findable.

## Sketch of the acceptance criteria

- **R1** — WHEN a cross-filesystem move is about to remove its source, `xdu-mv` SHALL first rename
  the source name, without replacing anything, to a private name in the same directory, and
  SHALL remove that private name only while it holds the inode that was copied.
- **R2** — IF the private name holds any other object, THEN `xdu-mv` SHALL put it back at the
  original name without replacing anything, withdraw its copy, and count the file skipped.
- **R3** — An interrupted run SHALL still leave every selected file whole at its source, its
  destination, or both, with any private name findable by its `.xdu-` prefix.
- **R4** — The link route of the no-replace rename SHALL get the same treatment, or say in
  `xdu-mv(1)` why it does not.

## Notes

- Related: `spec/bulk-copy-move-xdu-cp-mv/PLAN.md` §2.5 (`move_across`) and §5;
  `research/02-cross-filesystem-ci.md` §2 (the remaining races, and Linux
  `F_SETLEASE` as a separate guard against open writers); `issues/xdu-tar-slice-archive.md`
  (shares the copy primitive, not the unlink).
- Found by: `bulk-copy-move-xdu-cp-mv` plan (research 02), filed at P5.
