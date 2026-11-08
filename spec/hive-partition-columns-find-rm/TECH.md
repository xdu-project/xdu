---
slug: hive-partition-columns-find-rm
title: A key=value directory name reads like any other in xdu-find, xdu-rm, xdu-view
kind: fix
appetite: small
status: in_progress
branch: fix/hive-partition-columns-find-rm
base: main
current_phase: P2
last_updated: '2026-11-08'
phases:
- id: P1
  name: Hive detection off in every local reader; pins; six tests red on 052bbc7
  status: done
  satisfies:
  - R1
  - R2
  - R3
  - R4
  - R5
  - R7
  depends_on: []
  parallel: false
  hammerable: false
  hill: crest
  verify: set -eu; mkdir -p target; cargo test --locked --lib >target/p1-lib.log 2>&1;
    for t in tests::test_parquet_source_keeps_local_sql_byte_for_byte tests::test_parquet_source_binds_the_s3_glob
    view_query::tests::local_view_sql_is_byte_for_byte_what_xdu_view_sent view_query::tests::s3_view_sql_binds_the_glob_and_the_directory_prefix;
    do grep -qF "test $t ... ok" target/p1-lib.log; done; cargo test --locked --test
    find_tests --test rm_tests --test view_tests >target/p1-it.log 2>&1; for t in
    find_reads_a_key_value_partition_as_a_bare_name find_reads_an_index_under_a_key_value_directory_as_a_bare_one
    rm_reads_a_key_value_partition_as_a_bare_name rm_reads_an_index_under_a_key_value_directory_as_a_bare_one
    view_reads_a_key_value_partition_as_a_bare_name view_reads_an_index_under_a_key_value_directory_as_a_bare_one;
    do grep -qF "test $t ... ok" target/p1-it.log; done; sh spec/hive-partition-columns-find-rm/verify/scope.sh;
    sh spec/hive-partition-columns-find-rm/verify/red.sh; .agents/factory/bin/temp_index.sh
    sh -c 'set -eu; mkdir mtime=0 atime=0; cp -R "$XDU_INDEX" mtime=0/idx; cp -R "$XDU_INDEX"
    atime=0/idx; test "$(xdu-find --count)" -gt 0; test -z "$(xdu-find -i mtime=0/idx
    --mtime-older-than 3650)"; test "$(xdu-find -i mtime=0/idx --count)" = "$(xdu-find
    --count)"; xdu-rm -i atime=0/idx --older-than 3650 -n | grep -qx "No matching
    files found."'; cargo test --locked; cargo clippy --locked --all-targets --all-features
    -- -D warnings; cargo fmt --all -- --check; echo P1-GATE-GREEN
- id: P2
  name: 'S3 wire tests (count 56); AGENTS.md; prove the backport onto v0.6.0 + #36'
  status: in_progress
  satisfies:
  - R1
  - R3
  - R5
  - R6
  - R7
  depends_on:
  - P1
  parallel: false
  hammerable: false
  hill: crest
  verify: 'set -eu; mkdir -p target; listed=$(cargo test --locked --test s3_tests
    -- --list --include-ignored 2>/dev/null | grep -c '': test$''); test "$listed"
    -eq 56; for t in find_key_value_names_read_as_bare_names view_key_value_names_read_as_bare_names;
    do grep -B1 "^fn $t()" tests/s3_tests.rs | grep -qF ''#[ignore = "needs the pinned
    httpfs''; if sed -n "/^fn $t()/,/^}/p" tests/s3_tests.rs | grep -qw return; then
    echo "$t returns early" >&2; exit 1; fi; done; grep -qF ''sh tests/s3-wire.sh
    "" 56'' .github/workflows/test.yaml; grep -qF ''XDU_TEST_EXTENSION_DIR=target/test-ext
    sh tests/s3-wire.sh "" 56'' AGENTS.md; if grep -nF ''s3-wire.sh "" 54'' .github/workflows/test.yaml
    AGENTS.md; then echo ''a stale wire count remains'' >&2; exit 1; fi; sed -n ''/^##
    Project/,/^Seven binaries/p'' AGENTS.md | tr -s ''[:space:]'' '' '' >target/p2-project.txt;
    for w in xdu-find xdu-rm xdu-view ''select-act engine'' ''hive_partitioning =
    false''; do grep -qF "$w" target/p2-project.txt; done; if grep -qE ''do not yet|issues/hive-partition-columns-find-rm''
    target/p2-project.txt; then echo ''the Project paragraph still says do not yet
    or points at the seed'' >&2; exit 1; fi; tr -s ''[:space:]'' '' '' <.agents/factory/invariants.md
    | grep -qF ''read_parquet(?, hive_partitioning = false, filename=true)''; sh spec/hive-partition-columns-find-rm/verify/scope.sh;
    sh spec/hive-partition-columns-find-rm/verify/backport.sh; cargo test --locked;
    cargo clippy --locked --all-targets --all-features -- -D warnings; cargo fmt --all
    -- --check; echo P2-GATE-GREEN'
review:
  last_reviewed_commit: ''
  verdict: none
  blocked_reason: ''
  cycle: 0
---
# TECH.md — A `key=value` directory name reads like any other in `xdu-find`, `xdu-rm`, `xdu-view`

The **context engine and finite-state machine** for this fix. The YAML frontmatter above is the
resume ground-truth (read it with
`uv run --with pyyaml python .agents/factory/bin/next_phase.py spec/hive-partition-columns-find-rm/TECH.md`);
the per-phase checklists below are the work. `xdu-build` executes the next actionable phase, runs
its `verify:` command, updates state via
`uv run --with pyyaml python .agents/factory/bin/set_phase.py …`, and makes one atomic code+state
commit.

- **Vision / requirements (locked):** [`GOAL.md`](GOAL.md) — R-IDs are the contract.
- **Authoritative design:** [`PLAN.md`](PLAN.md). Lean path: no `research/`; the root-cause
  experiments and their before/after table are PLAN §4.

## Frontmatter field reference

As in [the template](../../.agents/factory/templates/TECH.md): `status`, `appetite`, phase
`status`, `satisfies`, `depends_on`, `parallel`, `hammerable`, `hill`, `attempts`, `verify`,
`review.cycle`. Both phases start at `hill: crest`: the cause and the cure were proven by
experiment at plan time (PLAN §4.1); what remains is execution and proof.

## Conventions (apply to every phase)

- Commit conventions, code style and load-bearing invariants come from
  [`AGENTS.md`](../../AGENTS.md); the curated checklist is
  [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) (§3, §4, §5, §11, §13).
  `src/lib.rs`, `src/bin/xdu-rm.rs` and `src/view_query.rs` are on the high-blast list.
- One phase per `xdu-build` invocation; one atomic commit with the code and this file's state.
  Subjects `[fix] Build hive-partition-columns-find-rm P<n>: …`, 72 characters at most. No
  `Co-Authored-By` trailer.
- No CLI surface change, so no `doc/*.scd` edit. No change to `src/select.rs`, `xdu-cp`, `xdu-mv`
  or `tests/common/mod.rs` (R6: #37's helpers sit there).
- **R6 discipline, both phases.** Product and test code may use only what exists at `v0.6.0` +
  `0547790`: never `select::`, `atomic_fs`, `relocate`, `XduCpArgs`/`XduMvArgs`/`SelectArgs`, or
  the `tests/common/mod.rs` helpers `build_index_block_size`, `append_bytes`, `run_cp`, `run_mv`,
  `run_with_stdin`. `verify/backport.sh` names these and the build at `v0.6.0` proves it.
- **No fixture name may contain `_` or `%`** (other than `__root__`, which the crawler makes):
  `LIKE` treats them as wildcards
  ([`issues/xdu-view-like-metacharacters.md`](../../issues/xdu-view-like-metacharacters.md)), which
  would make a view listing pass or fail for a reason that is not this defect.
- **Environment.** The gates build the bundled DuckDB; on a macOS host whose default SDK cannot
  compile it, export `SDKROOT` to a working SDK first. `verify/red.sh` and `verify/backport.sh`
  build into `target/red-target` (the first run compiles DuckDB there; later runs only the `xdu`
  crate). Neither fetches anything. The S3 wire tier needs the pinned httpfs, which this host does
  not have: P2 does not run it and does not fetch it; CI does (see P2, *Left for CI*).
- **The gate scripts** (`spec/hive-partition-columns-find-rm/verify/`, written at plan; each
  header states its contract):
  - `scope.sh` holds R4 and R7 structurally. No file under `src/` is added or removed. Every
    file under `src/`, test modules included, equals `052bbc7`'s once comment lines are dropped,
    every `, hive_partitioning = false` is deleted and whitespace is removed. The production
    option count is exactly 2 in `lib.rs`, 1 in `xdu-rm.rs` and 0 in `view_query.rs`. Every
    production `read_parquet(` outside `select.rs` carries the option. The old `partitions_sql`
    claim is gone.
  - `red.sh` holds R5 for the local arm. It runs this branch's `tests/` on `052bbc7`'s `src/`
    and requires each of the six named tests to run and fail with its token in its own failure
    section.
  - `backport.sh` holds R6 (P2).

  If a gate script is wrong, amend it in the phase commit and say why in the body; never weaken
  what its header promises.

---

## Phase P1 — Hive detection off in every local reader; pins; six tests red on `052bbc7`
**Satisfies:** R1, R2, R3, R4, R5 (local), R7 (the `src/` comment) · **Depends on:** —
**Goal:** `xdu-find`, `xdu-rm` and `xdu-view` read a local index whose partition, index directory
or ancestor has a `key=value` name exactly as a bare one. The statement pins carry the new text,
and each new test is shown to fail on `main`.

- [x] `src/lib.rs::parquet_source`: insert `, hive_partitioning = false` after the glob in both
      arms, before the optional `, filename=true` (PLAN §2.1). Rewrite its doc comment to say both
      arms turn Hive detection off and why. Change nothing else; `scope.sh` allows only the
      option and comment lines.
- [x] `src/bin/xdu-rm.rs`: the selection statement reads
      `read_parquet('{}', hive_partitioning = false)`, and the comment above it says why. Nothing
      else in the binary changes.
- [x] `src/view_query.rs`: the comment above `partitions_sql` no longer says Hive partitioning
      "would yield no partition column at all". It says Hive detection is off in
      `parquet_source`, so a `key=value` name reads as a name. No statement change.
- [x] Pins (PLAN §2.2), updated by exactly the option:
      `tests::test_parquet_source_keeps_local_sql_byte_for_byte` and
      `tests::test_parquet_source_binds_the_s3_glob` (expected strings), the frozen `old_*_sql`
      builders in `view_query::tests` (each `read_parquet('{glob}'` / `read_parquet('{}'`), and
      `s3_view_sql_binds_the_glob_and_the_directory_prefix`'s needles and exact string. Update the
      comments that describe them. Do not loosen any comparison and do not rename the tests.
- [x] `tests/find_tests.rs`: `find_reads_a_key_value_partition_as_a_bare_name` and
      `find_reads_an_index_under_a_key_value_directory_as_a_bare_one` (PLAN §2.3).
- [x] `tests/rm_tests.rs`: `rm_reads_a_key_value_partition_as_a_bare_name` and
      `rm_reads_an_index_under_a_key_value_directory_as_a_bare_one` (PLAN §2.3). Every run under
      a `path=` index is `-n`.
- [x] `tests/view_tests.rs`: `view_reads_a_key_value_partition_as_a_bare_name` and
      `view_reads_an_index_under_a_key_value_directory_as_a_bare_one` (PLAN §2.3).
- [x] In every new test the **first** assertion is the GOAL's named case, and its message
      contains the token `red.sh` requires: `key=value partition:` for the two partition
      tests, `key=value index directory:` for the two index-directory tests. Layout I's key list
      is `path size uid gid mode atime mtime ctime partition k`. Every other comparison is
      against a bare twin built from the same tree, never against a value re-derived from a
      reader.
- **Built (amendment):** two crawls of one tree need not store its rows in the same order, so
  the tests compare output lines in printed order only where the flags give a total order
  (`-f size`, `-f atime`, `--top` over fixture values that are all distinct; `SortMode::Name`
  for the views) and as sorted lines or name-keyed entries otherwise. The partition layout's
  twin is indexed from the same files before the directory is renamed `size-0` → `size=0`, so
  every field but the name, `ctime` included, is identical. `red.sh` needed no change.
- **Verify:** the frontmatter `verify:`. It requires the four pins and the six tests ok, then
  `scope.sh`, then `red.sh` (all six red on `052bbc7` with their tokens). Then a CLI drive in
  `temp_index.sh`: the throwaway index copied under `mtime=0/` and `atime=0/`;
  `--mtime-older-than 3650` prints nothing there, `--count` equals the bare index's, and `xdu-rm
  --older-than 3650 -n` finds nothing. Then full `cargo test`, clippy and fmt.
- **Touches:** `src/lib.rs`, `src/bin/xdu-rm.rs`, `src/view_query.rs` (all **high blast**),
  `tests/find_tests.rs`, `tests/rm_tests.rs`, `tests/view_tests.rs`.

## Phase P2 — S3 wire tests (count 56); `AGENTS.md`; prove the backport onto `v0.6.0` + #36
**Satisfies:** R1, R3, R5 (S3), R6, R7 (`AGENTS.md`) · **Depends on:** P1
**Goal:** the `s3://` arm of `xdu-find` and `xdu-view` is held by two wire tests on the
siblings' terms. The docs say every reader turns Hive detection off. The product+test diff is
shown to apply and pass on the patch-release base.

- [x] `tests/s3_tests.rs`: `find_key_value_names_read_as_bare_names` and
      `view_key_value_names_read_as_bare_names` (PLAN §2.4). Each has `#[test]` and the
      siblings' exact `#[ignore = "…"]` reason. The S3 prefix carries `mtime=0` (find) or
      `atime=0` (view) and the local copy sits at a bare path. The tree has a `size=0`
      partition. The absolute assertions on the S3 arm come first, then
      `Parity::assert_same` / `ViewParity::compare`, then `finish()`. No `return` anywhere in
      the bodies (the wire script refuses an early one). A constructor taking a source builder
      may be added to `Parity`/`ViewParity`. Pad with `wide_source_tree` if `finish()` refuses a
      whole-object GET.
- [x] Wire count 54 → 56 in the same commit: `.github/workflows/test.yaml`
      (`sh tests/s3-wire.sh "" 56`) and `AGENTS.md`
      (`XDU_TEST_EXTENSION_DIR=target/test-ext sh tests/s3-wire.sh "" 56`).
- [x] `AGENTS.md` *Project* paragraph (R7, main-only): every reader (`xdu-find`, `xdu-rm`,
      `xdu-view` and the select-act engine) reads with `hive_partitioning = false`, so a
      `key=value` name is read as a name. Drop "do not yet" and the seed pointer.
- [x] Lockstep, main-only: `invariants.md` §5. `parquet_source`'s text becomes
      `read_parquet(?, hive_partitioning = false)` (or
      `read_parquet(?, hive_partitioning = false, filename=true)`). Its local pin is named as now
      carrying the option. The sentence on why Hive detection is off covers every reader, held
      also by this cycle's six local tests and two wire tests. §3 still says every reader goes
      through the index globs.
- [x] Deferral ledger: walk P1, P2 and PLAN for "not fixed here", "follow-up", "known
      limitation" and "deferred". This plan defers nothing. Any deferral found while building gets
      its own `issues/` file (from `templates/ISSUE.md`) and a `ROADMAP.md` entry before this
      phase closes; never `META.md`. PLAN §5's README and site observation is a question for the
      foreman, not a deferral.
- [x] Run `verify/backport.sh` last, on the finished working tree (it is in the `verify:`). Do
      not touch any path it rejects as neither `src/`, `tests/` nor main-only; a man page or the
      README would need an explicit decision first.
- **Built (run A, 2026-11-08):** both wire tests share `key_value_tree`: `varied_source_tree`'s
  1400 files (sizes 1 to 1400 bytes, at most 140 days old) under the one partition `size=0`, plus
  `big.bin` (5,000,000 bytes, accessed a minute ago, so the newest file). The bucket copies sit
  under `kv/mtime=0/idx` (find) and `kv/atime=0/idx` (view); the local index sits at a bare path.
  `Parity` gained `over` (as `ViewParity` has), so a test seeds its own tree; `Parity::new`
  delegates to it. The 1401-row chunk is not padded with `wide_source_tree`: the siblings'
  `finish()` already passes on `bob`'s 450-row chunk from the same generator, so a whole-object
  GET is not expected (CI confirms). Distinct sizes keep `-f size` a total order. The absolute
  assertions and the view's exact entry count (`7 * (1 + 2 * 1406)`) were checked on the local
  arm with a scratch, uncommitted copy of each test against indexes stored under
  `kv/mtime=0/idx` and `kv/atime=0/idx`: green with the fix, and red at the first assertion
  (`-u size=0 --max-size 1K`) with `hive_partitioning = false` removed from `parquet_source`.
  The ledger walk found no deferral; PLAN §5's README and site observation goes to the foreman.
- **Status after run A:** code, docs and every local check are done and green; the phase stays
  `in_progress` until CI shows the wire tier. **Left for run B:** the Tests workflow's "S3 tests"
  step on both legs must print `s3-wire: '' 56 of 56 passed` with both new tests `ok` in its log,
  and the rest of the workflow green on the pushed head; then mark P2 `done`.
- **Verify:** the frontmatter `verify:`, in order:
  - 56 tests listed under `--include-ignored`; each new test carries the ignore reason and no
    `return`.
  - Both count literals at 56, and no stale 54.
  - The *Project* paragraph names the four readers and the option, without "do not yet" or the
    seed pointer; `invariants.md` carries the new `parquet_source` text.
  - `scope.sh` again.
  - `backport.sh`: a scratch worktree at `v0.6.0` with `git cherry-pick --no-commit 0547790`,
    then `git apply` of `git diff 052bbc7 -- src tests` with no fuzz, then full `cargo test` there
    with the six tests and four pins ok. It also checks that no added line names #37's symbols
    and that every other changed path is main-only, then removes the worktree.
  - Full `cargo test`, clippy and fmt.
- **Touches:** `tests/s3_tests.rs` (possibly `tests/common/s3.rs`, unchanged by #37),
  `.github/workflows/test.yaml` (**CI gate**), `AGENTS.md`, `.agents/factory/invariants.md`.
- **Left for CI:** the Tests workflow's "S3 tests" step on both legs must print
  `s3-wire: '' 56 of 56 passed`. This is the only execution of the two wire tests (R5 accepts CI
  evidence for that arm). Their red-on-`main` is by construction (PLAN §2.4), not observed.
  `backport.sh` reports, as information only, whether the `test.yaml` hunk also applies at
  `v0.6.0` + #36. `test.yaml` is not part of R6's restricted diff, and the wire script passes
  with a minimum below the listed count, so `/xdu-release` may carry or drop it.

---

## How `xdu-build` drives this

1. `next_phase.py` prints the next actionable phase.
2. Pre-flight: clean tree, on `branch`, `base` reachable.
3. Execute every `[ ]` in the phase (PLAN.md for detail).
4. Run the phase's `verify:` command — never advance on a checkbox alone.
5. Amend this file if reality diverges (regenerate frontmatter with `set_phase.py`; note it in the
   commit body). STOP and escalate only on a `GOAL.md` contradiction.
6. Mark the phase `done`, advance `current_phase`, `--touch`; one `[fix]` commit; stop and report.
