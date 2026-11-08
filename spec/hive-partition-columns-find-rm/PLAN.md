# PLAN — A `key=value` directory name reads like any other in `xdu-find`, `xdu-rm`, `xdu-view`

> **Status:** Draft for review · **Last updated:** 2026-11-08
> **Authoritative technical design.** The *how*. Vision/contract is [`GOAL.md`](GOAL.md);
> the phased executable roadmap is [`TECH.md`](TECH.md). Lean path (`kind: fix`, `appetite:
> small`, root cause known): no `research/`; the experiments that settle the unknowns are in §4.
> Every design element traces to a GOAL R-ID.

## 1. Summary

Turn DuckDB's Hive auto-detection off in the two places a `xdu-find`, `xdu-rm` or `xdu-view`
statement names its source: `lib::parquet_source` (both arms; `xdu-find` and every `xdu-view`
builder take their `FROM` from it) and `xdu-rm`'s own selection statement. The change is the
constant option `hive_partitioning = false` inserted into three format strings, the same option
the select-act engine already uses. Experiment confirms it is the whole cause and the whole cure
for the local readers (§4). The rest of the cycle is proof: regression tests that fail on `main`
and pass with the fix, the statement pins updated by exactly the option, S3 wire tests for the
`s3://` arm, and a gate that applies the product+test diff to `v0.6.0` + #36 and runs the suite
there (R6). Two phases.

## 2. Design

### 2.1 The fix (three format strings)

- **`src/lib.rs::parquet_source`.** The local arm becomes
  `read_parquet('<glob>', hive_partitioning = false[, filename=true])` and the S3 arm
  `read_parquet(?, hive_partitioning = false[, filename=true])`. The option is a constant: no
  value reaches SQL text that did not before, and the S3 glob stays the one bound value (R4, §5).
  The option goes before `filename=true`, so `xdu-view`'s and `xdu-find --top`'s partition
  extraction from the `filename` column is untouched. The doc comment says why the option is
  there (a `key=value` name every globbed path carries would otherwise become a column shadowing
  the row's own).
- **`src/bin/xdu-rm.rs`.** The selection statement becomes
  `SELECT path, size, atime FROM read_parquet('{}', hive_partitioning = false) {} {}`. Nothing
  else in the binary changes: the `WHERE`, the `deterministic_limit_clause` `ORDER BY path` tail,
  `--dry-run`, the prompt, `--safe` and the S3 refusal are as they were (R2, §4).
- **`src/view_query.rs`.** No statement change: every builder already takes its source from
  `parquet_source`. The comment above `partitions_sql` drops "would yield no partition column at
  all" and says instead that Hive detection is off in `parquet_source`, so a `key=value` name is
  read as a name (R7).

**Alternative considered: route `xdu-rm` through `parquet_source`.** It would put the option in
one place. Rejected for this cycle: it rewrites the statement in the destructive binary rather
than inserting a constant, so the scope gate (§6) could no longer prove that the only code change
anywhere under `src/` is the option. The unification belongs to
[`issues/xdu-rm-on-shared-engine.md`](../../issues/xdu-rm-on-shared-engine.md), a GOAL non-goal.

### 2.2 Statement pins (R4)

Updated by exactly the option, never relaxed:

- `src/lib.rs` `tests::test_parquet_source_keeps_local_sql_byte_for_byte`: both expected strings
  gain `, hive_partitioning = false` before the closing parenthesis or `, filename=true`. The
  comment above them says the pin now carries the Hive option; the test keeps its name, which
  `invariants.md` §5 cites.
- `src/lib.rs` `tests::test_parquet_source_binds_the_s3_glob`: `read_parquet(?,
  hive_partitioning = false)` and `read_parquet(?, hive_partitioning = false, filename=true)`.
- `src/view_query.rs` `tests`: each frozen `old_*_sql` builder's `read_parquet('{glob}'` (and
  `read_parquet('{}'` in the partition-root one) gains the option, and the comment above them
  says so. `s3_view_sql_binds_the_glob_and_the_directory_prefix`'s `contains(...)` needles and
  its exact `partition_root_sql` string gain it too. The placeholder count is unchanged.

`verify/scope.sh` holds this structurally: every file under `src/`, test modules included,
equals `052bbc7`'s once comment lines are dropped, the option text is deleted and whitespace is
removed. So nothing but the option and comments changed.

### 2.3 Local regression tests (R1, R2, R3, R5)

Six tests, two per tool, each against indexes the real `xdu` builds (`build_index`, so sizes are
apparent), each with a **bare twin** built from the same tree. Each one's *first* assertion is
the GOAL's named case, and its message carries a stable token (`key=value partition:` or
`key=value index directory:`) that `verify/red.sh` requires on `main`. Only fixtures that exist at
`v0.6.0` + #36 are used (`build_index`, `create_test_file`, `run_find`, `run_rm`,
`set_atime_days_ago`, `set_mtime_days_ago`, `set_atime_at`); none of #37's helpers (R6). No
helper is added to `tests/common/mod.rs`, where #37's additions sit.

**Layout P (partition).** A source whose one top-level directory is `size=0`, holding
`big.bin` (5,000,000 bytes), a 1-byte `small.txt` and `sub/mid.txt` (a few hundred bytes, so the
views have a directory to list), and a twin source whose partition is `size-0`. No fixture name
holds `_` or `%`, which `LIKE` reads as wildcards
([`issues/xdu-view-like-metacharacters.md`](../../issues/xdu-view-like-metacharacters.md)). A
single-partition index is the case that breaks even without `-u`.

**Layout I (index directory).** One fresh tree with two partitions and a loose file
(`__root__`), indexed once to `<work>/bare/index` and once to `<work>/<key>=0/index` for every
key in `path size uid gid mode atime mtime ctime partition k` (the eight columns, the
`partition` alias the views and `--top` group by, and a key naming no column). Row paths do not
depend on where the index is stored, so each tool's output must be **byte-identical** to the
bare index's.

- `tests/find_tests.rs`
  - `find_reads_a_key_value_partition_as_a_bare_name` — first: `-u size=0 --max-size 1K` prints
    exactly the paths of `small.txt` and `sub/mid.txt` (no `big.bin`). Then the same without
    `-u`; `--min-size 1M` prints exactly `big.bin`;
    `-f size` puts `big.bin` first; and for `[]`, `--count`, `--top 5`, `-f csv`, `-f size`,
    `-f atime`, `--max-size 1K`, `--min-size 1M`, with and without `-u`, the output equals the
    twin's with `size-0` mapped to `size=0`.
  - `find_reads_an_index_under_a_key_value_directory_as_a_bare_one` — first: under `mtime=0`,
    `--mtime-older-than 3650` prints nothing while the files are seconds old. Then for every key,
    each of `[]`, `--count`, `--top 5`, `-f csv`, `-f size`, `-f atime`, `-u <partition>`,
    `-p '*.txt'`, `--regex -p 'txt$'`, `--min-size 1`, `--max-size 1K`, `--older-than 3650`,
    `--newer-than 1`,
    `--mtime-older-than 3650`, `--mtime-newer-than 1`, `--owner` and `--group` with the
    fixture's numeric uid and gid, and `--mode` with a spec the fixture matches prints exactly
    what the bare index prints.
- `tests/rm_tests.rs`
  - `rm_reads_a_key_value_partition_as_a_bare_name` — first: `-u size=0 --max-size 1K -n`
    previews exactly `small.txt` and `sub/mid.txt`. Then `--min-size 1M -n` previews exactly
    `big.bin`; `-p '*.txt' -n`, `--limit 1 -n` and `--max-size 1K --safe -n` equal the twin's
    (mapped); and the real run `-u size=0 --max-size 1K --force` removes the two small files and
    leaves `big.bin` on disk, with and without `--safe` (a fresh fixture and index per real run).
  - `rm_reads_an_index_under_a_key_value_directory_as_a_bare_one` — first: under `atime=0`,
    `--older-than 3650 -n` prints `No matching files found.` while the files are fresh. Then the
    real run `--older-than 3650 --force` there deletes nothing. Then for every key, `-n` with no
    filter, `-p '*.txt'`, `--older-than 3650`, `--newer-than 1`, `--min-size 1`, `--max-size 1K`,
    `-u <partition>` and `--limit 1` prints exactly what the bare index's `-n` prints.
    **Every run under a `path=` index is a dry run**: on `main` the shadowed `path` column holds
    the directory's value, so a real run would unlink that relative name in the test's working
    directory (§4.1).
- `tests/view_tests.rs` (calls the `xdu::view_query` functions `xdu-view` calls)
  - `view_reads_a_key_value_partition_as_a_bare_name` — first: `partitions` reports `size=0`
    with `total_size` 5,000,001 (from the fixture's sizes, not from a reader). Then
    `partitions` under every `SortMode`, `partition_root`, `list_directory` and
    `tree_directory` from the partition root down, `root_files`, each under no filter, a `*.txt`
    path pattern, `with_max_size("1K")`, `with_min_size("1M")` and `with_older_than(3650)`,
    equal the twin's
    with the partition name mapped.
  - `view_reads_an_index_under_a_key_value_directory_as_a_bare_one` — first: under `atime=0`,
    a listing with `with_older_than(3650)` is empty and the unfiltered listing's `latest_atime`
    equals the fixture's (set with `set_atime_at` from one epoch). Then for every key, the same
    calls as above equal the bare index's exactly.

### 2.4 The S3 arm (R1, R3, R5)

The S3 writer admits `=` in a partition name
(`s3::tests::partition_names_that_survive_as_keys_and_globs_are_accepted` accepts `a+b=c,d;e`),
and the locator refuses only glob metacharacters in a prefix, so both layouts fit the wire
harness. Two `#[ignore]`d wire tests in `tests/s3_tests.rs`, on the
siblings' terms (the exact ignore reason, `Hermetic`, no early `return`, `finish()`):

- `find_key_value_names_read_as_bare_names` — seeds the S3 copy of an index whose tree has a
  `size=0` partition (`big.bin` 5,000,000 bytes, files under 1K) under a prefix carrying
  `mtime=0` (e.g. `kv/mtime=0/idx`), while the local copy sits at a bare path. Absolute
  assertions on the S3 run first: `-u size=0 --max-size 1K` lists exactly the small files, and
  `--mtime-older-than 3650` prints nothing. Then `Parity::assert_same` over `[]`, `--count`,
  `--top 5`, `-f csv`, `-u size=0`, `-u size=0 -f size`, `--max-size 1K`, `--min-size 1M`.
- `view_key_value_names_read_as_bare_names` — the same tree under a prefix carrying `atime=0`;
  absolute assertions first (`size=0`'s `total_size` from the fixture; `latest_atime` equals the
  fixture's), then `ViewParity::compare` over the partition list under every sort and the
  directories of `size=0`.

On `main` both are red by construction: the prefix's `mtime=0`/`atime=0` shadows only the S3
arm, so parity fails, and the absolute assertions fail on both arms for `size=0`. A constructor
taking a source builder may be added to `Parity`/`ViewParity`. If a chunk is smaller than
DuckDB's footer read, `ViewParity::finish` refuses the whole-object GET: pad with
`wide_source_tree` as #36 did. The wire count rises 54 → 56 in `.github/workflows/test.yaml` and
`AGENTS.md` in the same commit (AGENTS.md, Testing).

### 2.5 Documentation (R7, main-only)

- `AGENTS.md` *Project* paragraph: every reader (`xdu-find`, `xdu-rm`, `xdu-view` and the
  select-act engine) reads with `hive_partitioning = false`; drop "do not yet" and the seed
  pointer.
- Lockstep (`invariants.md` must track `AGENTS.md`): `invariants.md` §5's description of
  `parquet_source`'s text (`read_parquet(?)` / `read_parquet(?, filename=true)`) gains the
  option, and the paragraph naming why Hive detection is off covers all readers, held by this
  cycle's six local tests and two wire tests besides `cp_tests`/`mv_tests`.
- No man page, README or CLI change: no flag or output changes. The README's raw DuckDB examples
  are not touched (§5).

### Requirement → design map

| R-ID | Design element(s) that satisfy it |
|------|-----------------------------------|
| R1   | `parquet_source` option (both arms) §2.1; `find_*` local tests §2.3; `find_key_value_names_read_as_bare_names` wire test §2.4 |
| R2   | `xdu-rm` statement option §2.1; `rm_*` tests §2.3 (dry run, real run, `--limit`, `--safe`); S3 refusal untouched |
| R3   | `parquet_source` option (every `view_query` builder) §2.1; `view_*` local tests §2.3; `view_key_value_names_read_as_bare_names` §2.4 |
| R4   | Constant option only; S3 glob still bound; pins updated by exactly the option §2.2; `verify/scope.sh`; existing suites green |
| R5   | Six local tests, red on `052bbc7` (`verify/red.sh`); two wire tests, CI evidence §2.4 |
| R6   | No #37 dependency or overlap (§4.4); `verify/backport.sh` applies the `src/`+`tests/` diff onto `v0.6.0` + `0547790` and runs `cargo test` there (P2 gate) |
| R7   | `partitions_sql` comment §2.1 (backports); `AGENTS.md` *Project* paragraph §2.5 (main-only) |

## 3. Invariant gate (AGENTS.md constitution check)

Checked against [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) before
the probes and again after this design. Touched: §3, §4, §5, §11, §13. High-blast files touched:
`src/lib.rs`, `src/bin/xdu-rm.rs`, `src/view_query.rs`, so any CONFIRMED review finding forces the
human gate.

- **§3 Partition scheme** — the glob still comes only from `index_glob`/`index_glob_s3`; the
  option is outside the glob. Layout, `__root__` and reserved names untouched.
- **§4 `xdu-rm` destructive safety** — only the source text changes. `--limit` keeps its
  `ORDER BY path` tail, so `--dry-run` and the real run still select identical rows; `--safe`
  still re-checks exactly `--older-than` and `--max-size` (GOAL non-goal); prompt, `--force`,
  empty-set exit 0 and the S3 refusal unchanged. The selected set changes only for an index whose
  globbed paths all carry a `key=value` name, where it becomes the correct one.
- **§5 Injection surface** — no user value enters SQL text; the option is a literal. The local
  raw-glob exception is unchanged (binding it is a non-goal). The S3 arm still binds the whole
  glob as the one value, and `xdu-view`'s S3 arm still binds the prefix. The pins are updated by
  exactly the option (`scope.sh`).
- **§11 Altitude** — the reader seam stays in `lib`; tests drive real binaries and the
  `view_query` functions.
- **§13 Conventions** — the wire count rises in the commit that adds the wire tests;
  `invariants.md` is edited with `AGENTS.md`. No CLI change, so no `doc/*.scd` edit.

### Deviation justifications

| Deviation | Why needed | Simpler alternative rejected because |
|-----------|-----------|--------------------------------------|
| —         | —         | — |

## 4. Rabbit holes (resolved)

Settled by experiment on `c8e5b11` (= `052bbc7` for `src/`), DuckDB 1.4.4, driving the real
debug binaries against throwaway indexes, and by a scratch integration test calling the
`view_query` functions; then the same probes with the option inserted in a scratch worktree.

### 4.1 Hive detection is the cause, for all three tools

| Probe | `main` | with the option |
|-------|--------|-----------------|
| partition `size=0` (5,000,000 B + 1 B), `xdu-find -u size=0 -f csv` | both rows `size` 0 | 5000000, 1 |
| same, `xdu-find -u size=0 --max-size 1K` | lists both | `small.txt` only |
| same, no `-u` (single-partition index) | lists both | `small.txt` only |
| same, `xdu-rm -u size=0 --max-size 1K -n` | previews both | `small.txt` only |
| mixed index (`size=0` + bare `plain`), `xdu-find --max-size 1K` | correct (detection needs every path) | correct |
| index under `mtime=0/`, fresh file, `xdu-find --mtime-older-than 3650` | matches; csv `mtime` 0 | nothing; real mtime |
| index under `atime=0/`, `xdu-rm --older-than 3650 -n` | previews the fresh file | `No matching files found.` |
| index under `path=zzz/`, `xdu-find` / `xdu-rm -n` | prints and previews `zzz` | the real path |
| index under `foo=bar/`, `xdu-find -f csv` | correct (explicit column list) | correct |
| `view_query`, partition `size=0`: `partitions`, `list_directory`, `tree_directory` | `total_size` 0 | 5000001 / 5000000 / 1 |
| `view_query`, index under `atime=0/` | `latest_atime` 0; `older_than(3650)` lists the fresh file | real atime; empty |
| `view_query`, index under `partition=q/` | `partitions` fails: binder error, `filename` must appear in `GROUP BY` | correct |
| `view_query`, index under `path=zzz/` | `partition_root` empty; listings empty | correct |

Two findings beyond the seed. A `key` naming no column does add a column (the GOAL's reasoned
claim): `partition=` collides with the alias `partitions_sql` and `xdu-find --top` group by, and
breaks both. And `path=` shadows the path itself, so `xdu-rm` previews (and a real run would
unlink) the directory's value as a relative name in its working directory; the rm tests keep
every `path=` run dry for that reason (§2.3).

### 4.2 The option keeps the `filename` partition extraction working

With `hive_partitioning = false, filename=true` the partition list and `--top` read the
partition from `filename` exactly as on a bare index (probe rows above). The local statement pins
pass with the option inserted (`cargo test --lib`, 218 passed on the probe).

### 4.3 The S3 arm

`parquet_source`'s S3 arm sends `read_parquet(?)` with the bound glob and no Hive option, so
DuckDB's detection runs on the expanded key list the same way: a `key=value` segment in the prefix
or in a `-u` partition shadows the column. Not run here (the pinned httpfs is not present
locally); held by reading and by the two wire tests, which run in CI. The writer admits `=` in
partition names and the locator admits it in a prefix (§2.4).

### 4.4 R6 is reachable with no overlap

Of the files this fix touches, #37 changed only `src/lib.rs`'s module list (`pub mod atomic_fs;`,
`relocate`, `select`), far from `parquet_source`; `src/bin/xdu-rm.rs`, `src/bin/xdu-find.rs`,
`src/view_query.rs`, `tests/find_tests.rs`, `tests/rm_tests.rs`, `tests/view_tests.rs`,
`tests/s3_tests.rs` and `tests/common/s3.rs` are identical at `0547790` and `052bbc7`, and
`Cargo.lock` is identical at `v0.6.0`. The probe's `src/` diff passed `git apply --check` onto a
scratch worktree at `v0.6.0` with `git cherry-pick --no-commit 0547790`.

## 5. Risks & open questions

- **The S3 arm runs only in CI.** Its red-on-`main` is by construction (§2.4), not observed; its
  green is the CI "S3 tests" step printing `s3-wire: '' 56 of 56 passed` on both legs. If
  httpfs or the loopback endpoint mishandles `=` in a key, CI goes red for a harness reason; the
  build cannot see that locally, and the review must read CI.
- **`path=` on `main`.** A red run of the rm tests on `main` with a real deletion under a `path=`
  index would unlink a relative name. Held by keeping those runs dry (§2.3) and by `red.sh`
  running in a scratch tree.
- **Outside the GOAL, for the foreman:** the README's raw DuckDB examples
  (`read_parquet('/var/lib/xdu/home/*/*.parquet')`) have the same shadowing for a user who
  queries such an index by hand, and the project site calls the index "Hive-partitioned". Neither
  is in this GOAL's scope and this plan defers nothing to them; whether to seed them is the
  foreman's call.

## 6. Verification strategy

The three gate scripts in `verify/` are committed with this plan, so the build is graded by
gates written before its code; their headers state their contracts. `scope.sh` was exercised
against the probe patch (green), a relaxed S3 needle (red) and an `xdu-rm` left unfixed (red);
`backport.sh` ran end to end on the probe patch: it applied onto `v0.6.0` + `0547790`, the full
suite passed there, the worktree was removed, and it failed only on the six tests the probe did
not have. `red.sh` is checked for syntax and its failure-section parsing only; its first real run
is P1's gate.

- **P1 (local):** the four pins and the six new tests named ok; `verify/scope.sh` (the only
  `src/` change is the option, the pins moved by exactly it, every production `read_parquet(`
  outside `select.rs` carries it); `verify/red.sh` (the six tests run on `052bbc7`'s `src/` and
  each fails with its token); a CLI drive in `temp_index.sh` (the throwaway index copied under
  `mtime=0/` and `atime=0/`: `xdu-find --mtime-older-than 3650` prints nothing, `--count` equals
  the bare index's, `xdu-rm --older-than 3650 -n` finds nothing); full `cargo test`, clippy, fmt.
- **P2 (S3, docs, backport):** the two wire tests listed under `--include-ignored` with the
  siblings' ignore reason and no `return`; listed count 56 and both count literals; the
  `AGENTS.md` and `invariants.md` greps; `scope.sh` again; `verify/backport.sh` (restricted diff
  applies onto `v0.6.0` + `0547790` with no fuzz, full `cargo test` passes there, the six tests
  and four pins ok, nothing #37 added is named, every other changed path is main-only); full
  `cargo test`, clippy, fmt. Left for CI: the wire tier at 56.

---

*Lean path: no `research/`.*
