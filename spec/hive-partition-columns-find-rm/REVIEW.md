# REVIEW — A `key=value` directory name reads like any other in `xdu-find`, `xdu-rm`, `xdu-view`

> Adversarial QA by `xdu-review`, run in an isolated/clean context. The correctness pass grades the
> branch diff against [`GOAL.md`](GOAL.md) + the AGENTS.md invariants **only** — it does not see
> `PLAN.md`/`TECH.md` (avoids grading-its-own-homework / plan-sycophancy). Every finding cites an
> **executed** command, not an assertion.

- **Reviewed commit:** `bcb5b27` (the last code commit is `5ef684b`; `bcb5b27` changes only
  `spec/`)  ·  **Base:** `main` (`052bbc7`)  ·  **Date:** 2026-11-08
- **Verdict:** approved (0 CONFIRMED, 0 PLAUSIBLE). Human gate not triggered.
- **Cycle:** 1 of ≤3 — mirrors `review.cycle` in `TECH.md` (escalate to human on non-convergence)

Mode: **debate**, a fresh blind pass over the whole diff with `spec/` excluded. Two reviewers ran in
parallel: one argued "ship" (S), the other "block" (B). Each was handed, inline, GOAL.md in full, the
full text of `invariants.md` and `review-rubric.md`, and the diff commands with `spec/` excluded. Both
were barred from PLAN, TECH, `research/`, META, everything under this spec, and the build's leftover
files under `target/`. Each had its own `CARGO_TARGET_DIR` and TMPDIR under `target/review-{ship,block}/`.
The completeness sub-pass was not requested.

Contract drift: after the shaping commit `86ab80a`, GOAL.md changed once, in `c8e5b11`. That commit
records the clarifications on release scope and on how R6 is measured, approved by the foreman
before planning. This review grades GOAL.md as it stands at `bcb5b27`.

Every R-ID was gradable from the diff with `spec/` excluded. None is a committed artifact under
`spec/`: R7's `AGENTS.md` text and the `partitions_sql` comment are both in that diff.

## Verification run

Both reviewers ran each item below, unless an item names one of them. The orchestrator re-read both
reviewers' saved test logs to reconcile the counts.

- **Head, `cargo test --locked`:** 353 passed, 0 failed, 55 ignored (52 S3 wire, 3 xfs). Both logs
  give the same count for every test binary: lib 218, `cp_tests` 33, crawl 25, `mv_tests` 21,
  `rm_tests` 21, find 11, version 6, `xdu-view` bin 5, `extension-probe` 4, s3 4 run with 52 ignored,
  view 3, offline 1, `cli_version_tests` 1.
- **Lint and format:** `cargo clippy --locked --all-targets --all-features -- -D warnings` exits 0,
  and so does `cargo fmt --all -- --check`.
- **The six new local tests fail on `main`.** The setup is a scratch worktree at `052bbc7` with
  `git diff 052bbc7...HEAD -- tests` applied. It compiles clean. All six new tests fail there, each
  on a shadowed value:
  - `find_reads_a_key_value_partition_as_a_bare_name`: `-u size=0 --max-size 1K` lists
    `size=0/big.bin`.
  - `find_reads_an_index_under_a_key_value_directory_as_a_bare_one`: under `mtime=0`,
    `--mtime-older-than 3650` matches files written seconds earlier.
  - `rm_reads_a_key_value_partition_as_a_bare_name`: `-n` previews `size=0/big.bin`.
  - `rm_reads_an_index_under_a_key_value_directory_as_a_bare_one`: under `atime=0`,
    `--older-than 3650 -n` previews files accessed minutes earlier.
  - `view_reads_a_key_value_partition_as_a_bare_name`: `total_size: 0` where it should be 5000301.
  - `view_reads_an_index_under_a_key_value_directory_as_a_bare_one`: `latest_atime: 0`.

  All six pass at the head.
- **CLI drives at the head**, with `temp_index.sh` and the reviewers' own throwaway trees:
  - The GOAL examples hold. `xdu-find -u size=0 --max-size 1K` and `xdu-rm -u size=0 --max-size 1K
    -n` list or preview only the small file. A real `xdu-rm --force` run leaves the 5,000,000-byte
    `big.bin` in place. Under `mtime=0/idx`, `--mtime-older-than 3650 --count` gives `0`. Under
    `atime=0/idx`, `xdu-rm --older-than 3650` prints `No matching files found.` with `-n`, and again
    with `--safe --force`.
  - **S:** 427 comparisons of `key=value` layouts against bare names: 0 differences at the head, and
    69 with `main`'s binaries as the negative control.
    - Layouts: an index under each of the eight column keys and an unknown key, an index directory
      named `mtime=0`, `x=1/y/size=9`, and a partition `size=0` against its `size-0` twin.
    - Flags, with and without `-u`: `--count`, `--top`, csv/long/size/atime output, every size,
      age and mtime filter, `-p`, `--regex`, `--mode` and `--limit`. On the `xdu-rm` side: `-n`
      with its filters, `--limit` and `--safe`.
  - **B:**
    - An index under `atime=0/path=zz/foo=bar/idx` reads like bare names.
    - A partition `filename=x` under a `filename=y/` directory (keys that collide with the
      `filename=true` column) gives correct `--top` and `-f size`.
    - A real `xdu-rm --force --safe` run on partition `path=victim`, with a decoy `./victim` in the
      working directory, deleted only the selected file and kept `big.bin` and the decoy.
  - **Bare names, head against `main`'s binaries:** S compared 12 `xdu-find` and 5 `xdu-rm -n` flag
    sets byte for byte and found them identical. B saw differences only in row order among ties
    (see *Observations*).
- **R6 backport:**
  - Setup: a scratch worktree at `v0.6.0`, then `git cherry-pick --no-commit 0547790`, then `git
    apply` of `git diff 052bbc7...HEAD -- src tests`. It applied without conflict (line offsets
    only).
  - `cargo test --locked` there: 251 passed, 0 failed, 52 ignored. Both logs give the same count for
    every test binary. The six new local tests and the three exact-text pins pass, and the two new
    wire tests compile and are ignored. There are no cp or mv suites at that base.
  - S's report summed the run to 247. Its own log gives 251, the same as B's, and the orchestrator
    took the count from the logs.
- **S3 wire tier.** It was **not observed** on this host, because the pinned httpfs test extension
  is absent. Both reviewers graded the S3 arm by reading:
  - `find_key_value_names_read_as_bare_names` and `view_key_value_names_read_as_bare_names` run on
    the same terms as their siblings (`#[ignore]` with the httpfs reason, Parity/ViewParity, then
    `finish()`).
  - They put partition `size=0` under the prefix `kv/mtime=0/idx` or `kv/atime=0/idx`. The S3
    writer admits `=` in a partition name.
  - Each checks the fixture over S3 before comparing it with the local copy, so each would fail
    without the fix.
  - Both compile, and the ignored count went from 50 to 52.
  - CI evidence, observed by the foreman on the PR rollup for `5ef684b`: CLEAN, every job
    successful. Both legs report `s3-wire: '' 56 of 56 passed`, with
    `find_key_value_names_read_as_bare_names ... ok` and `view_key_value_names_read_as_bare_names
    ... ok`.
  - CI on `bcb5b27` was **not observed** by this review. The foreman is watching it.
- **Man pages.** The diff touches no `doc/*.scd` and no CLI flag, so no page text changed. `scdoc`
  is not installed on this host, so no local render was run. The man-page gate was green on
  `5ef684b`, inside the rollup above. On `bcb5b27` it was **not observed**.

## Requirement → evidence matrix

| R-ID | Implemented by | Verified how | Who | Status |
|------|----------------|--------------|-----|--------|
| R1 | `parquet_source` (`src/lib.rs:199`, `:205`), which every `xdu-find` statement uses (`src/bin/xdu-find.rs:68`) | the two new `find_tests` (fail at `052bbc7`, pass at head); the CLI drives above; S3 arm by reading + CI fact | S, B | ✅ (S3 arm: not observed locally; CI green on `5ef684b`) |
| R2 | `src/bin/xdu-rm.rs:89` | the two new `rm_tests` (fail at `052bbc7`, pass at head); `-n` and real `--force`/`--safe` drives; S3 refusal unchanged (`test_s3_locator_is_refused_before_anything_runs`) | S, B | ✅ |
| R3 | `parquet_source`, which every `view_query` builder uses (`src/view_query.rs:28/94/184/233`); `src/bin/xdu-view.rs` sends no SQL of its own | the two new `view_tests` (fail at `052bbc7`, pass at head): five builders × every `SortMode` × 5 filter sets × 10 keys against a bare twin; S3 arm by reading + CI fact | S, B | ✅ (S3 arm: not observed locally; CI green on `5ef684b`) |
| R4 | the full-string pins (`test_parquet_source_keeps_local_sql_byte_for_byte`, `test_parquet_source_binds_the_s3_glob`, `local_view_sql_is_byte_for_byte_what_xdu_view_sent`) updated to the exact new text, not loosened; `s3_view_sql_binds_the_glob_and_the_directory_prefix` keeps its existing `contains`/`assert_eq` form, now with the option | head suite 353/0; bare-name outputs head vs `main`; the only new SQL text is the constant option, so no user value is added (§5) and the S3 glob stays bound | S, B | ✅ |
| R5 | six local tests in `tests/{find,rm,view}_tests.rs`; two wire tests in `tests/s3_tests.rs`; `s3-wire.sh` count 54→56 | red-on-`main` control (six fail on a shadowed value); green at head; wire tests by reading + CI fact | S, B | ✅ (wire arm: CI evidence, as R5 allows) |
| R6 | the diff restricted to `src/` and `tests/` | applies to `v0.6.0`+`0547790` without conflict; 251/0 there; relies only on `set_atime_at` (#36), `set_times`, `list_files_recursive` and `ViewParity` (pre-#37); #37 changed only the `pub mod` lines among the files this diff touches, and this diff leaves them alone | S, B | ✅ |
| R7 | `AGENTS.md` *Project* paragraph; comment above `partitions_sql` | read at head: names all four readers as reading with `hive_partitioning = false`, drops the "do not yet" seed pointer; the comment no longer says "would yield no partition column at all" | S, B | ✅ |

Unmapped changes (possible scope creep): none. Both reviewers mapped the hunks outside `src/` and
`tests/` as follows:
- `invariants.md` §5, the `AGENTS.md` Commands count, and `.github/workflows/test.yaml`
  (54→56): obligations the new reader rule and the two new wire tests carry into the same commit.
  Every test name they cite exists.
- `issues/hive-partition-columns-find-rm.md`: only the seed's status line changes, recording its
  promotion. It is main-only.

## Findings

None. Neither reviewer reported a CONFIRMED or a PLAUSIBLE finding, so the two agree completely.
Candidates they refuted and dropped:
- Differences between head and `main` on bare names: the differing lines were ties in row order, and
  the same order changes from run to run on `main` (see below).
- The CI wire count in a backport: `.github/` is outside R6's restricted diff, and at `v0.6.0`+#36
  `test.yaml` already reads `54`, so the hunk carries over.
- `temp_index.sh` rebuilding the release binaries mid-review: they did not change, and the checksums
  taken before and after are identical.

## Observations (out of scope; not findings)

- `xdu-find --top`, `-f size` and `-f atime` print rows that tie on the sort key (same size, same
  atime, same count) in an order that changes between runs of the *same* binary, on `main` and at
  the head alike (B, by repeated runs and md5). The order of tied rows was never stable, so R4's
  "same order as on `main`" cannot be held to it, and it is not this diff's regression. It is
  recorded here for the human to seed if wanted. `xdu-rm`'s `--limit` selection is unaffected: it
  orders deterministically (§4).
- For `/xdu-release`: the S3 wire count in `.github/workflows/test.yaml` sits outside R6's
  restricted diff. A `v0.6.1` branch must carry the `54`→`56` change too, either by applying this
  hunk, which fits at `v0.6.0`+#36, or by adding 2 to its own count (S, B).

## Human-gate triggers

- Not triggered: there is no CONFIRMED finding. The diff does touch high-blast-radius files
  (`src/bin/xdu-rm.rs`, `src/lib.rs`, `src/view_query.rs`) and the §5 text, and that is why this
  review ran as a debate. The gate fires only on a CONFIRMED finding.

## Hygiene

- `git status --porcelain` is empty, and `git worktree list` shows only the main checkout and the
  cycle worktree. Every reviewer scratch worktree was reset clean and removed with `git worktree
  remove`, never with `--force`.
- `target/release/{xdu,xdu-find,xdu-rm,xdu-view,xdu-cp,xdu-mv}` are byte-identical to the
  orchestrator's checksums from before launch (`shasum -c`: all OK). Each reviewer wrote only under
  its own `target/review-*/`.
- Processes: the sandbox blocks `ps`, so no process could be listed directly. Both reviewers report
  that every background job they started completed and that no server or daemon was left running.
  The wire-tier servers did not run, and the local test servers run in-process.
