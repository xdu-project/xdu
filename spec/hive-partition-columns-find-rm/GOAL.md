# GOAL — A `key=value` directory name reads like any other in `xdu-find`, `xdu-rm`, `xdu-view`

> **Origin spec.** The *what* and *why* — the locked contract `xdu-review` grades against.
> The *how* lives in [`PLAN.md`](PLAN.md) and [`TECH.md`](TECH.md) (written by `xdu-plan`).
> Promoted from [`issues/hive-partition-columns-find-rm.md`](../../issues/hive-partition-columns-find-rm.md),
> which remains the pre-shaped evidence record. Do not treat the seed's draft R-IDs or its candidate
> fix as settled — this file is the contract.

- **slug:** hive-partition-columns-find-rm
- **kind:** fix
- **appetite:** small

## Problem

The index layout is `<index>/<partition>/NNNNNN.parquet` with bare directory names, but nothing
stops a top-level source directory from being named `size=0`, or an index from being stored under a
directory such as `…/mtime=0/`. DuckDB's `read_parquet` auto-detects Hive partitioning: when every
path it globs carries a directory named `key=value`, it adds a column `key` holding `value`, and
that column shadows the file's column of the same name. Every row read through such a glob then
reports `size` 0, or `mtime` 0, whatever the crawler recorded, and every filter, count, sum and
sort on that column works on the wrong value. Without `-u`, a mixed index usually reads correctly,
because detection needs every globbed path to carry the key; a single-partition index, a `-u`
scope, or an index path carrying a `key=value` component breaks every query.

The evidence at `main` `052bbc7`: `lib::parquet_source` (`src/lib.rs:183`) writes
`read_parquet('<glob>'[, filename=true])` for a local index (line 191) and `read_parquet(?[,
filename=true])` with the glob bound for an `s3://` index (line 195), with no Hive option on either.
`xdu-find` takes every statement's source from it (`src/bin/xdu-find.rs:68`), and so does each
`xdu-view` statement builder in `src/view_query.rs` (lines 27, 92, 182, 231). `xdu-rm` builds its
own `SELECT path, size, atime FROM read_parquet('{}') …` from `index_glob` (`src/bin/xdu-rm.rs:61`,
86–89), local only: it refuses an `s3://` index (line 30). The raw `read_parquet('{glob}')`
statements at `src/view_query.rs:448–605` sit inside its `#[cfg(test)]` module (line 429) and are
not production readers. The comment above `partitions_sql` (`src/view_query.rs:84–86`) says Hive
partitioning "would yield no partition column at all", which is true of the bare layout but not of a
`key=value` name. The select-act engine already reads with `hive_partitioning = false`
(`src/select.rs:273`, 391, 401) and is out of scope here.

The seed reproduced it on a local index during the `bulk-copy-move-xdu-cp-mv` review: with a
5,000,000-byte file under the partition `size=0`, `xdu-find -u size=0 --max-size 1K` lists it and
`xdu-rm -u size=0 --max-size 1K -n` previews deleting it; an index under a `mtime=0` directory makes
`xdu-find --mtime-older-than 3650` match files written seconds earlier. `xdu-rm` is the dangerous
case: it deletes the rows the shadowed column selects, so a size or age filter meant to protect
files does not. One correction to the seed: `xdu-rm` has no `--mtime-*` filters. Its age filters
(`--older-than`, `--newer-than`) compare `atime`, and `--safe` re-stats each file and re-checks
only `--older-than` and `--max-size` before the unlink (`src/bin/xdu-rm.rs:138–180`); by reading,
not execution, a shadowed `size` defeats `--min-size`, which `--safe` does not re-check. Nothing in
the mechanism limits it to `size` and `mtime`: any of the eight column names, `path` included, has
the same shape, and a `key` matching no column adds one (reasoned, for `/xdu-plan` to confirm).
**Pre-existing**: present on `main` and on the released `v0.6.0` (`src/bin/xdu-rm.rs:87` there, and
`parquet_source` with no Hive option).

## Outcome / vision

Every reader `xdu-find`, `xdu-rm` and `xdu-view` use, for a local index and an `s3://` index alike,
sees exactly the columns the crawler wrote. Naming a source directory, an index directory, or any
directory above the index `key=value` changes nothing a user observes. The fix is small and
self-contained enough to ship as a patch release on top of `v0.6.0` and #36 without the
unreleased `xdu-cp`/`xdu-mv` work.

## Acceptance criteria (the contract)

"A `key=value` name" below means a directory named `<column>=<value>` for any of the eight index
column names (`path`, `size`, `uid`, `gid`, `mode`, `atime`, `mtime`, `ctime`), and also one whose
key names no column. "As for bare names" means the same output for the same files crawled into an
index whose partition and index directories carry no `=`.

- **R1** — WHEN `xdu-find` reads a local or `s3://` index in which a partition, the index directory,
  or a directory above it has a `key=value` name, `xdu-find` SHALL list, count, sum, sort and group
  by partition exactly as for bare names, for every filter it offers, with and without `-u`. In
  particular, with a 5,000,000-byte file under a partition named `size=0`, `xdu-find -u size=0
  --max-size 1K` SHALL NOT list it, and for an index stored under a `mtime=0` directory,
  `--mtime-older-than 3650` SHALL NOT match a file modified within that window.
- **R2** — WHEN `xdu-rm` reads a local index in which a partition, the index directory, or a
  directory above it has a `key=value` name, the set it previews with `--dry-run` and the set it
  deletes SHALL each be exactly the set it selects for bare names, for every filter, `--limit` and
  `--safe` combination, with and without `-u`. In particular, `xdu-rm -u size=0 --max-size 1K -n`
  SHALL NOT preview the 5,000,000-byte file of R1, and the real run SHALL NOT delete it. `xdu-rm`
  SHALL still refuse an `s3://` index as it does today.
- **R3** — WHEN `xdu-view` reads a local or `s3://` index in which a partition, the index directory,
  or a directory above it has a `key=value` name, its partition list, directory listings, tree view,
  sizes, file counts and access times SHALL be exactly those it shows for bare names, under every
  filter set and sort mode.
- **R4** — WHEN no directory on an index's paths has a `key=value` name, `xdu-find`, `xdu-rm` and
  `xdu-view` SHALL return the same rows in the same order as on `main` `052bbc7`, and the existing
  find, rm, view and S3 tests SHALL pass. A unit test that pins reader statement text exactly
  (for example `lib.rs`'s `parquet_source` test and `view_query.rs`'s
  `local_view_sql_is_byte_for_byte_what_xdu_view_sent`) SHALL still pin the exact new text: updated
  deliberately, not relaxed. The fix SHALL NOT write into SQL text any user value that `main` binds
  or does not already write there (invariant §5); every value the S3 arm binds today SHALL stay
  bound.
- **R5** — Regression tests SHALL drive the shipped readers against an index `xdu` builds: for
  `xdu-find` and `xdu-rm`, a partition named `size=0` holding a file larger than the `--max-size`
  bound, and an index stored under a `key=value` directory with an age filter on the column it
  shadows (`mtime` for `xdu-find`, `atime` for `xdu-rm`); for `xdu-view`, the same two layouts
  through the `xdu::view_query` functions it calls. Each SHALL fail on `main` `052bbc7` and pass
  with the fix. The `s3://` arm of `xdu-find` and `xdu-view` SHALL be covered in the existing S3
  wire harness (`tests/s3_tests.rs`) on the same terms as its siblings, with the `key=value`
  directory placed where the S3 writer admits it (in the prefix, if it refuses `=` in a partition
  name). The S3 wire tier runs in CI only (the pinned test extension is not present locally), so
  CI evidence is acceptable for that arm.
- **R6** — The patch release `v0.6.1` is `v0.6.0` plus #36 (`0547790`) plus this fix, cherry-picked
  in that order, without the unreleased `xdu-cp`/`xdu-mv` work (#37). The landed change's diff,
  restricted to product code and tests (`src/`, `tests/`, and any doc that exists at `v0.6.0`
  without `xdu-cp`/`xdu-mv` text), SHALL apply without conflict onto `v0.6.0` with #36 applied, and
  SHALL build and pass there. That restricted diff MAY rely on #36's `src/view_query.rs` changes,
  its view helpers in `tests/common/mod.rs` and its S3 view tests. It SHALL NOT rely on anything #37
  introduced: `src/select.rs`, `src/atomic_fs.rs`, `src/relocate.rs`, the `xdu-cp` and `xdu-mv`
  binaries and their CLI structs, `cp_tests`/`mv_tests`/`mv_xfs_tests`, their docs, or the helpers
  #37 added to `tests/common/mod.rs` (`build_index_block_size`, `append_bytes`, `run_cp`, `run_mv`,
  `run_with_stdin`), and it SHALL change as few lines that #37 also changed as practical. The
  `AGENTS.md` edit (R7) and the `spec/` and `issues/` files are main-only and are not backported.
- **R7** — `AGENTS.md`'s *Project* paragraph SHALL say that every reader (`xdu-find`, `xdu-rm`,
  `xdu-view` and the select-act engine) turns DuckDB's Hive detection off, and SHALL drop the "do
  not yet" pointer to the seed. This edit is main-only: it is not part of R6's restricted diff and
  is not backported. The comment above `partitions_sql` SHALL no longer say a Hive reading "would
  yield no partition column at all"; it is in `src/`, so it is part of R6's diff and backports.

## Non-goals (no-gos)

- Moving `xdu-rm` onto the shared select-act engine. Owned by
  [`issues/xdu-rm-on-shared-engine.md`](../../issues/xdu-rm-on-shared-engine.md); doing it here would
  also make the fix depend on #37 (R6).
- Binding the local index path and partition names as statement parameters, which retires the §5
  local exception. Owned by
  [`issues/local-reader-bound-parameters.md`](../../issues/local-reader-bound-parameters.md).
- Any change to `src/select.rs` or to `xdu-cp`/`xdu-mv`: already fixed, and out of the backport.
- Refusing or renaming `key=value` source directories at crawl time. The names are legitimate; the
  readers must read them as names.
- Cutting the `v0.6.1` release itself (tagging, cherry-picking, publishing). That is
  `/xdu-release`'s job after this cycle lands; R6 only makes it possible.
- Making `xdu-rm --safe` re-check `--min-size` or any other filter it does not re-check today.

## Clarifications

- **Q:** Which readers are in scope? — **A:** Every reader that `xdu-find`, `xdu-rm` and
  `xdu-view` use, for local and S3-hosted indices alike. The cp/mv engine (`src/select.rs`) is
  already fixed and out of scope. Foreman ruling under the SC26 mandate; approved order relayed by
  the director (resolved 2026-11-08).
- **Q:** Is the contract the seed's mechanism (`hive_partitioning = false` on every call)? — **A:**
  No. The R-IDs state observable broken→fixed behaviour, per the `kind: fix` rule: a `key=value`
  directory name in a source tree, or in the path where an index is stored, selects the same rows as
  any other name, for each of the three tools, with a regression test that fails on current `main`
  for `xdu-find` and `xdu-rm` and covers `xdu-view` if it is affected (it is: R3, R5). The mechanism
  is `/xdu-plan`'s choice. Foreman ruling under the SC26 mandate; approved order relayed by the
  director (resolved 2026-11-08).
- **Q:** Is there a release constraint? — **A:** Yes. The fix will be cherry-picked onto the
  `v0.6.0` tag for a patch release `v0.6.1`, which must not carry the unreleased `xdu-cp`/`xdu-mv`
  work (#37). Product code and tests must not depend on anything #37 introduced and should touch as
  few lines shared with #37's changes as practical; documentation that mentions cp/mv goes in a
  separable place (R6, R7). Foreman ruling under the SC26 mandate; approved order relayed by the
  director (resolved 2026-11-08). Refined by the release-scope rulings below.
- **Q:** What is out of scope? — **A:** Moving `xdu-rm` onto the shared engine, the bound-parameters
  rewrite, any change to `src/select.rs`, and the release itself. Foreman ruling under the SC26
  mandate; approved order relayed by the director (resolved 2026-11-08).
- **Q:** Does the seed's R4 (`AGENTS.md` names every reader as covered) stay? — **A:** Yes, small:
  `AGENTS.md`'s *Project* paragraph has the sentence it updates (R7). Foreman ruling under the SC26
  mandate; approved order relayed by the director (resolved 2026-11-08).
- **Q:** Does the seed's mtime case apply to `xdu-rm`? — **A:** No, as a matter of fact found at
  shaping: `xdu-rm` and `xdu-view` have no `--mtime-*` filters (`src/cli.rs`, `XduRmArgs` and
  `XduViewArgs`); their age filters compare `atime`. R1 keeps the mtime example for `xdu-find`; R2
  and R3 cover every filter each tool has, and R5 names the age column per tool (resolved
  2026-11-08).
- **Q:** Must the S3 arm have a test, or is reasoning enough? — **A:** A test, in the existing wire
  harness on the same terms as its siblings (R5). Proposed at shaping and kept by the foreman, who
  noted that the S3 wire tier currently runs in CI only (the pinned test extension is not present
  locally), so CI evidence is acceptable for that arm. Foreman ruling; release scope per the
  director's relayed answer of 2026-11-08 (resolved 2026-11-08).
- **Q:** Does `v0.6.1` carry #36 (the `xdu-view` code-point fix), which `v0.6.0..main` also holds?
  — **A:** Yes. The release is `v0.6.0` + #36 (`0547790`) + this fix, cherry-picked in that order,
  so R6's base is `v0.6.0` with #36 applied: the fix may rely on #36's `src/view_query.rs` changes,
  its `tests/common/mod.rs` view helpers and its S3 view tests, and still must not rely on anything
  #37 added. Foreman ruling; release scope per the director's relayed answer of 2026-11-08
  (resolved 2026-11-08).
- **Q:** How is R6 measured, given that `/xdu-publish` squash-merges and a separate docs commit
  does not survive? — **A:** On the landed change: its diff restricted to product code and tests
  (`src/`, `tests/`, and any doc that exists at `v0.6.0` without `xdu-cp`/`xdu-mv` text) applies
  without conflict onto `v0.6.0`+#36 and builds and passes there. The `AGENTS.md` *Project* edit
  (R7) and the `spec/` and `issues/` files are main-only and are not backported; the
  `partitions_sql` comment fix is in `src/` and backports. The "separate commit" mechanism is
  dropped from R6 and R7. Foreman ruling; release scope per the director's relayed answer of
  2026-11-08 (resolved 2026-11-08).

## Related materials

- Seed: [`issues/hive-partition-columns-find-rm.md`](../../issues/hive-partition-columns-find-rm.md)
- `src/lib.rs`: `parquet_source`, `index_glob`, `index_glob_s3`, and the unit test pinning
  `parquet_source`'s text
- `src/bin/xdu-rm.rs`: the selection statement and the S3 refusal; `src/bin/xdu-find.rs`: the
  `source` closure
- `src/view_query.rs`: `partitions_sql` and its comment, the other statement builders, and
  `local_view_sql_is_byte_for_byte_what_xdu_view_sent`
- `src/select.rs`: the engine's `hive_partitioning = false` (prior art; not to be touched or
  depended on)
- `tests/find_tests.rs`, `tests/rm_tests.rs`, `tests/view_tests.rs`, `tests/s3_tests.rs`
- [`spec/bulk-copy-move-xdu-cp-mv/REVIEW.md`](../bulk-copy-move-xdu-cp-mv/REVIEW.md), finding 3
- [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) §5
- Related seeds: [`xdu-rm-on-shared-engine`](../../issues/xdu-rm-on-shared-engine.md),
  [`local-reader-bound-parameters`](../../issues/local-reader-bound-parameters.md)
