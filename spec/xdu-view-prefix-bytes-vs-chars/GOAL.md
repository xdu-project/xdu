# GOAL — `xdu-view` lists full child names under a non-ASCII directory

> **Origin spec.** The *what* and *why* — the locked contract `xdu-review` grades against.
> The *how* lives in [`PLAN.md`](PLAN.md) and [`TECH.md`](TECH.md) (written by `xdu-plan`).
> Promoted from [`issues/xdu-view-prefix-bytes-vs-chars.md`](../../issues/xdu-view-prefix-bytes-vs-chars.md),
> which remains the pre-shaped evidence record. Do not treat the seed's draft R-IDs or candidate
> fixes as settled — this file is the contract.

- **slug:** xdu-view-prefix-bytes-vs-chars
- **kind:** fix
- **appetite:** small

## Problem

`xdu-view` lists a directory by matching the index rows under `<dir>/` and cutting the next path
component out of each row in SQL. The evidence at `main` `7689a92` (`src/view_query.rs`):
`DirectoryQuery::new` builds `prefix = format!("{}/", dir)` and records `prefix_len = prefix.len()`
(lines 232–235), a Rust byte count; `list_directory_sql` and `tree_directory_sql` splice that number
into `substr(path, {prefix_len} + 1)` (lines 293–298 and 342–347), and DuckDB's `substr` counts
characters. Confirmed at shaping on DuckDB 2.0.0 (the crate bundles 1.4.4; the plan re-confirms on
the bundled engine): `substr` and `length` both count Unicode code points, `strlen` counts bytes,
and `length_grapheme` is the separate grapheme count.

So every non-ASCII code point anywhere in the absolute directory path moves the cut one or more
characters too far right, and each child name loses that many leading characters. The prefix is the
whole absolute path, not the partition-relative part: a partition named `données`, a directory below
it, or the crawled root itself (an accented user name in a home directory) all trigger it. The seed
reproduced it on a local index: under `données/{alpha,beta,top}` both views list `eta`, `lpha`,
`op`. The damage is worse than mislabelling. `directory_entries` builds each entry's `path` from the
truncated name, so entering `lpha` looks under `<root>/données/lpha`, which matches no rows: the
subtree is unreachable. A name no longer than the overshoot becomes empty and drops out of the
listing entirely.

The index stores `path` as UTF-8 (`lib.rs`, the `path` field is `DataType::Utf8`). A name that is
not valid UTF-8 on disk is stored lossily, with U+FFFD (three bytes, one code point) in place of
each bad sequence, and the crawler counts it in `lossy_paths`. Such a stored name is therefore just
another non-ASCII string and carries the same defect. The S3 arm shares `DirectoryQuery` and the
same `substr` text, so an `s3://` index carries the defect for any non-ASCII directory in the stored
path (reasoned, not executed). Pre-existing: the byte count and the `substr` text date from the
views' first commits. `xdu-find` and `xdu-rm` have no such cut, and `xdu-view.rs`'s display slicing
of `current_path[partition_root.len()..]` compares Rust byte lengths on both sides and is not part
of this defect.

## Outcome / vision

Both views list the same children, under the same full names, under a non-ASCII directory as under
an ASCII one, and every listed directory can be entered, for a local index and an `s3://` index
alike. The offset that cuts a row and the prefix that selected it can no longer disagree about what
a "character" is. ASCII listings are unchanged, and the fix leaves the SQL injection surface no
wider than it found it.

## Acceptance criteria (the contract)

- **R1** — WHEN `xdu-view` lists a directory whose absolute path, as stored in the index, contains
  non-ASCII characters (in the crawled root, the partition name, or any directory below it), the
  list view and the tree view SHALL each report every child under its full stored name, with the
  same entries, sizes, file counts and latest access times they report for the same tree under an
  all-ASCII path.
- **R2** — WHEN the operator enters a directory listed under such a path, the view SHALL list that
  directory's own children, at every depth.
- **R3** — R1 and R2 SHALL hold for any valid UTF-8 the index stores, including names in decomposed
  form (a base letter plus a combining mark, one grapheme of several code points) and lossily
  stored names that carry U+FFFD.
- **R4** — R1 and R2 SHALL hold for a local index and for an `s3://` index, with the non-ASCII
  directory placed where each target admits it (for S3, above or below the partition, since the
  writer refuses a non-ASCII partition name).
- **R5** — A regression test SHALL call the `xdu::view_query` listing functions against an index
  `xdu` built from a tree with a non-ASCII directory above the listed one, and assert both views'
  names. It SHALL fail on `main` `7689a92` and pass with the fix; the S3 case SHALL run in the
  existing S3 wire-test harness on the same terms as its siblings.
- **R6** — WHEN every stored path is ASCII, both views SHALL return the same entries in the same
  order as on `main` for every filter set and sort mode, and the existing local and S3 view tests
  SHALL pass. The unit test that pins the local view statements byte for byte SHALL still pin the
  exact new text: updated deliberately, not relaxed to a looser check.
- **R7** — The fix SHALL NOT write into SQL text any string value that `main` binds or does not
  already write there (invariant §5; an integer computed in Rust is safe by type). Every value the
  S3 arm binds today SHALL stay bound.

## Non-goals (no-gos)

- Binding the local index path and partition names, and the `'`-in-a-directory-name failure of
  the local `LIKE` literal. Owned by
  [`issues/local-reader-bound-parameters.md`](../../issues/local-reader-bound-parameters.md); this
  fix need not retire the §5 local exception (see Clarifications for the one permitted overlap).
- Merging `list_directory_sql` and `tree_directory_sql` into one builder. That seed's ride-along.
- `LIKE` metacharacters (`_`, `%`) in a directory name over-matching a sibling (`a_b/` also
  selecting `axb/`). A distinct defect, noticed at shaping and not reproduced; if the plan confirms
  it, it is filed as a seed, not fixed here, unless the chosen design closes it at no extra scope.
- Recovering the original bytes of a non-UTF-8 name, or round-tripping a lossy name to disk. The
  view works on what the index stores.
- Unicode normalization. A name is listed exactly as stored; NFC and NFD spellings of one name stay
  two names.
- Display width, alignment or truncation of multibyte names in the terminal. Owned by
  [`issues/xdu-view-terminal-safety.md`](../../issues/xdu-view-terminal-safety.md).
- Non-ASCII S3 partition names. The writer refuses them;
  [`issues/s3-non-ascii-partition-names.md`](../../issues/s3-non-ascii-partition-names.md) owns
  that.
- `xdu-find`, `xdu-rm`, and the display slicing in `src/bin/xdu-view.rs`, which are consistent
  today.

## Clarifications

- **Q:** Does the fix count characters, or avoid positional cutting altogether? — **A:** Left to
  `/xdu-plan`; the contract is R1–R3 plus the requirement that the offset and the matched prefix
  be measured in the unit the cutting function uses, so they cannot disagree. The options on the
  table: a code-point count in Rust (`chars().count()` matches DuckDB's unit); computing the offset
  in SQL from the prefix itself (`length(<prefix>)`); or stripping the prefix with a prefix-aware
  function so no number is carried at all. The plan re-confirms on the bundled DuckDB that the
  functions it relies on count code points (resolved at shaping 2026-11-06).
- **Q:** Are multi-code-point graphemes and invalid-UTF-8 paths in scope? — **A:** Yes, as stored.
  Paths are UTF-8 in the index and invalid bytes arrive as U+FFFD, so both are ordinary non-ASCII
  strings to the view. Since `substr` and `length` share the code-point unit, a decomposed name
  cuts correctly once the units agree; R3 holds it with a test. Grapheme-aware cutting and byte
  recovery are not needed (resolved at shaping 2026-11-06).
- **Q:** What is the §5 stance? — **A:** Do not widen. The local arm writes the directory prefix
  into a `LIKE` literal today, a recorded §5 exception; the S3 arm binds it. A Rust-computed integer
  in SQL text is injection-safe by type (§5: numeric values are safe), so a character count adds no
  surface. Binding the local directory prefix, the value this fix touches, is a permitted option
  that narrows the exception. If the plan takes it, it binds the prefix only (not the index path or
  partition names), updates §5's `xdu-view` bullet and the `local-reader-bound-parameters` seed to
  say so, and keeps the local-statement pin exact (R6). Writing the prefix into a second literal
  that `main` does not have (for example a `starts_with('<prefix>')` beside the `LIKE`) is a
  widening and is not allowed (resolved at shaping 2026-11-06).
- **Q:** Can the S3 case be tested, given the writer refuses non-ASCII partition names? — **A:**
  Yes: the stored path is the crawled source path, so a non-ASCII directory below the partition, or
  above the crawled root, reaches the S3 arm. The existing wire harness (`tests/s3_tests.rs`,
  `view_quotes_in_the_prefix_and_a_directory_match_local` and siblings) is the place (resolved at
  shaping 2026-11-06).

## Related materials

- Seed: [`issues/xdu-view-prefix-bytes-vs-chars.md`](../../issues/xdu-view-prefix-bytes-vs-chars.md)
- `src/view_query.rs`: `DirectoryQuery::new`, `list_directory_sql`, `tree_directory_sql`,
  `directory_entries`; the unit test `local_view_sql_is_byte_for_byte_what_xdu_view_sent`
- `src/crawl.rs`: `lossy_path`, `lossy_paths`; `src/lib.rs`: the `path` field's `Utf8` type
- `tests/s3_tests.rs`: the `view_*` wire tests
- [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) §5
- Related seeds: [`local-reader-bound-parameters`](../../issues/local-reader-bound-parameters.md),
  [`xdu-view-terminal-safety`](../../issues/xdu-view-terminal-safety.md),
  [`s3-non-ascii-partition-names`](../../issues/s3-non-ascii-partition-names.md)
