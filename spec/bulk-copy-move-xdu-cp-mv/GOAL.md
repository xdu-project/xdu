# GOAL — Bulk copy and move: `xdu-cp` and `xdu-mv` over a shared select-act engine

> **Origin spec.** The *what* and *why* — the locked contract `xdu-review` grades against.
> The *how* lives in [`PLAN.md`](PLAN.md) and [`TECH.md`](TECH.md) (written by `xdu-plan`).
> Keep this at the right altitude: solved and bounded, but not over-specified — leave design
> freedom for the plan. Edit requirements here; do **not** silently drift them during build.

- **slug:** bulk-copy-move-xdu-cp-mv
- **kind:** feature
- **appetite:** big, bounded by the calendar: the cycle lands by **2026-11-13**, plan, build, CI
  and review included (about 6 working days, so about 3 of build)  ·  *two new tools on one engine
  lifted out of `xdu-rm`, local index and local files only. S3 as a source index, as a copy target
  and as a copy source are out (Non-goals). If the box tightens, the cut order in *Notes* applies,
  through a GOAL amendment, never a silent drop.*

## Problem

`xdu-rm` proved select-then-act: an index query names exactly the set, and dry-run, confirmation,
`--safe` re-stat and a deterministic `ORDER BY` under `--limit` guard the action. Relocation has
none of it. It goes through `xdu-find | xargs cp|mv`, where `xargs` splits the set across
invocations at `ARG_MAX`, the preview and the action can diverge without saying so, and a stale
index row moves a file that no longer matches anything. "Move everything untouched in two years to
cold storage" and "copy this user's stale logs to a staging prefix" are single safe commands
waiting to exist. Cold storage is almost always another filesystem, so a move is usually a copy
followed by a deletion at the source, and the deletion is as destructive as `xdu-rm`'s.

The pattern lives in one binary's `main` (`src/bin/xdu-rm.rs`, 245 lines), so a second and third
tool would copy it, gaps included. Confirmed against `7689a92`:

- `--safe` re-checks only atime (`--older-than`) and size (`--max-size`)
  (`src/bin/xdu-rm.rs:159-191`). `--min-size` and `--newer-than` are unchecked, as invariant §4
  records, and the man page claims more (`doc/xdu-rm.1.scd:64-67`); its own example
  `xdu-rm --min-size 1G --safe` (`:105`) re-checks nothing. The re-stat is `fs::metadata`, which
  follows a symlink planted at the path.
- The selection is `format!`-ed SQL with the local glob, index path and partition name written
  into a string literal (`src/bin/xdu-rm.rs:86`), the raw-interpolation exception of §5 in the
  one tool that deletes.
- `xdu-rm` exits 0 after failed unlinks (`:244`); the failures are only a summary line.
- `xdu-find` filters by owner, group, mode and mtime (`src/cli.rs:78-131`); `xdu-rm`'s flag set
  stops at pattern, partition, size and atime (`src/cli.rs:210-243`).
- Index rows hold absolute paths under the canonicalized crawl root (`src/bin/xdu.rs:612-615`),
  and the marker does not record that root, so a tree-preserving destination needs a root the
  operator names. A row whose name was not valid UTF-8 is stored lossily and names no real file
  (`src/crawl.rs:270-283`).

The archive tool sequenced next (`issues/xdu-tar-slice-archive.md`) needs the same selection and
the same staging copy, so the engine and the copy are built once, here.

## Outcome / vision

An operator runs `xdu-cp` or `xdu-mv` with the filters they already use with `xdu-find`, previews
the exact set and where each file will land, confirms, and gets exactly that set copied or moved
into a tree under `--into`, with every file either fully at its destination or untouched at its
source. `xdu-rm`, `xdu-cp` and `xdu-mv` share one library engine, so `xdu-rm` gains the `--safe`
checks it claimed and the bound-parameter selection, and `xdu-tar` starts from a selection, a path
mapping and a copy primitive that already exist.

## Acceptance criteria (the contract)

- **R1** — `xdu-rm`, `xdu-cp` and `xdu-mv` SHALL share one library implementation of
  select-then-act: the version gate and completion warning (§2b), the filter-to-`WHERE` build, the
  deterministic `ORDER BY path` under `--limit`, the empty-set message, dry-run, confirmation and
  `--force`, the `--safe` re-stat (R7), the parallel executor and the summary counts. The selection
  SHALL reach DuckDB with the index glob, including the index path and any partition name, as a
  bound parameter, so a `'` in either selects the right rows instead of failing or becoming SQL.
  Apart from R7 and that binding, `xdu-rm`'s flags, prompt, output and exit status SHALL be
  unchanged, and `tests/rm_tests.rs` SHALL pass with its assertions intact.
- **R2** — WHEN files match an index query, `xdu-cp -i INDEX --into DEST [--root ROOT]` SHALL copy
  exactly the matched set, each file to `DEST/<its path relative to ROOT>`, creating intermediate
  directories as needed; `--root` defaults to `/`, which reproduces each absolute path under `DEST`.
  Each copy SHALL carry the source's bytes, permission bits, atime and mtime, and its owner and
  group where the caller may set them, with a refusal counted in the summary, never silent.
  `xdu-cp` SHALL never modify or remove a source file. It SHALL act only on a regular file at the
  path, not following symlinks; anything else at the path is skipped and counted.
- **R3** — `xdu-cp` and `xdu-mv` SHALL exit non-zero with the cause on stderr, having written
  nothing, IF `--into` does not exist or is not a directory, IF the index locator is `s3://`
  (refused before any query, as `xdu-rm` refuses it), or IF any selected path is not under `--root`
  or maps to a destination equal to its own source path. The last two are checked over the whole
  selected set before the first file is acted on, and name the first offending path.
- **R4** — Neither tool SHALL replace or truncate an existing file at a destination path; such a
  file is skipped and counted as existing. A destination name SHALL never hold a partial file: a
  copy becomes visible at its final name only when complete, and a failed or interrupted copy
  leaves nothing at that name.
- **R5** — WHEN files match an index query, `xdu-mv` SHALL relocate exactly the matched set to the
  R2 layout, with R2's regular-file rule, R3's refusals and R4's no-overwrite rule. On one
  filesystem it SHALL rename without replacing an existing destination. Across filesystems it SHALL
  copy as R2 and R4 do, and unlink the source only after the copy is complete and flushed to stable
  storage and the source's size and mtime are unchanged since the copy began. IF any step fails, or
  the source changed during the copy, THEN the source SHALL remain in place, any partial copy SHALL
  be removed, and the file SHALL be counted as failed or skipped. An interrupted run SHALL leave
  every selected file whole at its source, its destination, or both; never at neither.
- **R6** — `xdu-cp` and `xdu-mv` SHALL carry §4's gates as `xdu-rm` has them: by default a `y/N`
  confirmation naming the file count and `--into`, where anything but `y`/`yes` aborts with nothing
  written; `--force` skips it; `--dry-run` prints each source with its destination and performs
  zero filesystem writes (no directory, no file, no unlink); an empty match prints a message and
  exits 0. WHEN `--limit` caps the set, `--dry-run` and the real run SHALL select identical rows.
- **R7** — WHEN `--safe` is given, each of the three tools SHALL `lstat` every file immediately
  before acting on it and skip it, counted, unless it is still a regular file and every filter
  given still holds against its live metadata: size against `--min-size`/`--max-size`, atime against
  `--older-than`/`--newer-than`, and, for `xdu-cp` and `xdu-mv`, mtime, owner, group and mode
  against their filters. The path pattern holds by construction. This closes §4's `xdu-rm` gaps
  without weakening its existing atime and size checks, and the `xdu-rm` man page SHALL then
  describe what is checked.
- **R8** — `xdu-cp` and `xdu-mv` SHALL accept every selection filter `xdu-find` accepts, with the
  same flags and semantics: `-p` glob or `--regex`, `-u`, `--min-size`, `--max-size`,
  `--older-than`, `--newer-than`, `--owner`, `--group`, `--mode`, `--mtime-older-than`,
  `--mtime-newer-than`, plus `-i`/`XDU_INDEX`, `-l`, `-n`, `--safe`, `-f`, `-v` and `-j`/`XDU_JOBS`
  as `xdu-rm` defines them. `xdu-rm`'s flag set is unchanged.
- **R9** — `xdu-cp` and `xdu-mv` SHALL end with a summary that counts each outcome separately
  (copied or moved, missing, existing at destination, skipped by `--safe` or the regular-file rule,
  ownership not preserved, failed), keep progress and per-file diagnostics off stdout except the
  `-v` listing and `--dry-run` output, and exit non-zero WHEN any file failed or already existed at
  its destination. Missing, skipped and ownership-not-preserved files are not failures.
- **R10** — `xdu-cp` and `xdu-mv` SHALL each have a man page (`doc/xdu-cp.1.scd`,
  `doc/xdu-mv.1.scd`) rendered and literal-checked by the man-page gate, shell completions from
  `src/cli.rs`, and a place in the release tarball, `install.sh` and the `hpccm/` recipe in
  lockstep (§13). Both pages SHALL state that the index is not updated, that reading a source may
  update its atime under the mount's options, and how that interacts with a later `--older-than`
  selection or `--safe`. `README.md` SHALL show one copy and one move.

## Non-goals (no-gos)

- **S3, every way round.** An `s3://` source index is refused (R3) for all three select-act tools,
  as `xdu-rm` refuses it today; the engine is local-only so it has one posture. Letting the engine
  read an S3 index folds into `issues/s3-index-rm.md`, widened at publish to name `xdu-cp` and
  `xdu-mv`. Copying or moving files into or out of an object store is not in the seed and is out.
- **Updating the index** after a move or copy, or marking moved rows. The index is a snapshot; a
  re-crawl refreshes it (R10 documents this).
- **Overwrite, update and resume modes**: `--overwrite`, `--update`, skip-if-identical, `--backup`.
  An existing destination is always skipped and counted (R4, R9). A re-run after an interrupted
  `xdu-mv` therefore reports existing files and exits non-zero, which is the safe answer.
- **Flattening** the tree, renaming patterns, or more than one `--root`.
- **Directory metadata**: replicating source directory modes, owners or times onto created
  directories, and pruning source directories a move empties. The index holds only regular files.
- **Extended attributes, ACLs, sparse-file preservation, hard-link preservation** across the set
  (each hard-linked name is copied as its own file), and reflink or `copy_file_range`
  optimisations beyond what the plan picks for correctness.
- **Preserving a source's atime on read** (`O_NOATIME`, restoring it after the copy). Documented,
  not changed (R10).
- **New filters for `xdu-rm`.** Its flag set stays as it is (R8); widening it is a follow-up.
- **Changing `xdu-rm`'s exit status** on failed unlinks. `xdu-cp` and `xdu-mv` exit non-zero on
  failure (R9); bringing `xdu-rm` in line is a behavior change on a high-blast file, filed as a
  follow-up seed at publish.
- **Bound parameters for `xdu-find` and `xdu-view`'s local SQL.** R1 binds only the engine's
  selection; the readers stay with `issues/local-reader-bound-parameters.md`, which this cycle
  narrows.
- **Lossily stored paths.** A row whose name was not valid UTF-8 names no real file and counts as
  missing, as in `xdu-rm`; storing raw bytes is an index-format change.
- **Everything `xdu-tar` owns**: archive formats, streaming, compression, stdout output, member
  order beyond `--limit`'s, live-tree directory and symlink capture
  (`issues/xdu-tar-slice-archive.md`), and index diffing
  (`issues/index-diff-incremental-select.md`).
- **Throughput tuning** beyond `-j` parallelism, and progress bars.

## Clarifications

- **Q:** Pilot or the whole seed? — **A:** The whole seed's tool pair, local only, cut to the
  calendar: both tools, the shared engine, `--safe` against the richer schema, the full filter
  surface. S3 and every convenience mode go to Non-goals. Set by the foreman's brief (appetite big,
  land by 2026-11-13) (resolved 2026-11-06).
- **Q:** May the source index be an `s3://` index? — **A:** No, in this cycle, for all three tools
  on the engine (R3). Shaping decision on the foreman's brief, which left it open: one engine with
  one S3 posture, matching `xdu-rm`'s refusal, and the reader-side wiring (bound S3 glob, marker
  read over S3, credential resolver) belongs to the follow-up that brings `xdu-rm` to S3
  (resolved 2026-11-06, gate to confirm).
- **Q:** What is the destination layout, and what root does it strip? — **A:** `DEST/<path
  relative to --root>`, `--root` defaulting to `/`, because the index records absolute paths and
  no crawl root. A selected path outside `--root` refuses the run before any write (R2, R3).
  Shaping decision (resolved 2026-11-06, gate to confirm).
- **Q:** Collision policy? — **A:** Never overwrite; skip, count, exit non-zero (R4, R9). The
  seed left it to shaping (resolved 2026-11-06).
- **Q:** `xdu-mv`: rename or copy-plus-unlink, and what across filesystems? — **A:** A
  no-replace rename on one filesystem; across filesystems a complete, flushed copy, a check that
  the source did not change during it, then the unlink (R5). Cross-filesystem is the cold-storage
  case, so it is in scope, not refused. The seed left it to shaping (resolved 2026-11-06).
- **Q:** Which columns does `--safe` check? — **A:** Every filter given, against live `lstat`
  metadata, plus the regular-file test; the pattern holds by construction (R7). For `xdu-rm` this
  covers only its own filters and closes §4's gaps (resolved 2026-11-06).
- [NEEDS CLARIFICATION: R2's owner and group preservation. Shaped as attempted, with a refusal
  counted (as `cp -p`): an administrator moving users' files to cold storage needs it, and a move
  by rename keeps ownership anyway. CI runs unprivileged, so only the success path is testable
  there; the refusal arm would be a unit test of the counting, not of a real `EPERM`. Gate: keep
  as shaped, or cut to a Non-goal and document that a cross-filesystem move leaves files owned by
  the caller?]
- [NEEDS CLARIFICATION: R7 brings `xdu-rm --safe` onto the shared check now, a behavior change on
  `src/bin/xdu-rm.rs` (more files skipped, never fewer). Shaped as in scope because one check
  serving three tools cannot keep `xdu-rm`'s gaps without a special case. Gate: accept, or keep
  `xdu-rm`'s current checks and file the gap as its own fix seed?]
- [NEEDS CLARIFICATION: Ten R-IDs sits at the size circuit-breaker's edge. Gate: accept the cut
  order in *Notes* as the pre-agreed fallback, or split now (for example `xdu-cp` plus the engine
  this cycle, `xdu-mv` next)?]

## Notes

- **Cut order if the box tightens**, each through a GOAL amendment: (1) R2's ownership
  preservation, to a Non-goal; (2) R7 for `xdu-rm`, to a fix seed, with `xdu-cp`/`xdu-mv` keeping
  the full check; (3) R1's bound-parameter selection, to `issues/local-reader-bound-parameters.md`.
  `xdu-mv`'s cross-filesystem path (R5) is the reason for the tool and is not on the list.
- **Phase order suggestion for `/xdu-plan`:** engine lifted with `xdu-rm` parity first, so
  `tests/rm_tests.rs` proves the refactor before anything is built on it; then `xdu-cp` and the copy
  primitive; then `xdu-mv`; documentation and packaging last.
- **Footguns the plan must design against, not contract terms:** `rename(2)` silently replaces an
  existing destination, so R4 and R5 need a no-replace primitive on both Linux and macOS; R5's
  cross-filesystem path must be testable in CI, whether on two real filesystems or by driving the
  library path directly; "every file whole somewhere" (R5) has to hold for a crash between any two
  steps.
- **What `xdu-tar` will reuse:** the engine's selection (gates, filters, `ORDER BY`, dry-run,
  confirmation, `--safe`) separable from the parallel executor, because an archive consumes one
  ordered stream; the `--root` path mapping and its outside-root refusal; R2's copy primitive with
  R4's atomic appearance, as the staging path its R3 weighs against streaming; and R9's counted
  outcomes. Tar's own member order, root handling and output are its own shaping.

## Related materials

- Seed: `issues/bulk-copy-move-xdu-cp-mv.md` (draft R1–R6, taken as input). GitHub issue #1.
- Sibling, sequenced next: `issues/xdu-tar-slice-archive.md`; third:
  `issues/index-diff-incremental-select.md`. `ROADMAP.md`, "Bulk operations" entries.
- Prior art: `src/bin/xdu-rm.rs`; `tests/rm_tests.rs`; `doc/xdu-rm.1.scd`;
  `spec/rm-limit-deterministic-order/` (the `ORDER BY path` rule);
  `lib::deterministic_limit_clause`, `lib::QueryFilters`, `lib::index_glob`, `lib::parquet_source` (`src/lib.rs`).
- Invariants: `.agents/factory/invariants.md` §2b (gates), §4 (destructive safety), §5 (SQL
  injection surface), §8 (regular files only), §10 (CLI single source), §11 (thin binaries), §13
  (tarball, `install.sh`, `hpccm/`, man-page literal counts).
- Follow-ups touched: `issues/s3-index-rm.md` (widened to the engine),
  `issues/local-reader-bound-parameters.md` (narrowed to the readers).
