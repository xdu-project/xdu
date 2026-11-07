# REVIEW — Bulk copy and move: `xdu-cp` and `xdu-mv` over a shared select-act engine

> Adversarial QA by `xdu-review`, run in an isolated/clean context. The correctness pass grades the
> branch diff against [`GOAL.md`](GOAL.md) + the AGENTS.md invariants **only** — it does not see
> `PLAN.md`/`TECH.md` (avoids grading-its-own-homework / plan-sycophancy). Every finding cites an
> **executed** command, not an assertion.

- **Reviewed commit:** `5c7f2b5` (code identical to the pushed head `769255e`; `5c7f2b5` adds only
  `evidence/ci.md`)  ·  **Base:** `main` (`0547790`)  ·  **Date:** 2026-11-07
- **Verdict:** changes-requested (4 CONFIRMED: 1 HIGH, 3 MEDIUM; 0 PLAUSIBLE). **Human gate
  triggered.**
- **Cycle:** 1 of ≤3 — mirrors `review.cycle` in `TECH.md` (escalate to human on non-convergence)

Mode: **debate**. Two fresh blind reviewers ran in parallel: A argued "ship" and B argued "block".
Each got GOAL.md inline, the diff commands with `spec/` excluded, and its own `git archive` export of
HEAD without `spec/`. Both were barred from PLAN, TECH, `research/`, META and `spec/`, and both
reported reading nothing there. `review-rubric.md` and `invariants.md` were passed by path inside
each export, `cmp`-identical to HEAD's copies, rather than pasted. Two short GOAL sections, the
phase-order note and the related-materials links, were left out of the inline copy. The
completeness sub-pass was skipped at the foreman's direction.

Contract drift: after the shaping commit `ecb2864`, `edcc163` records the GOAL-gate rulings and
`3ce7375` the plan-gate rulings. Both are the foreman's approved rulings, and this review grades
GOAL.md as it stands at `5c7f2b5`.

No R-ID is a committed artifact under `spec/`, so none was out of the reviewers' reach. The
orchestrator owns the CI rollup and the cut checkpoint.

## Verification run

**Orchestrator (O).** Export of `5c7f2b5` without `spec/`, with its own `CARGO_TARGET_DIR` and
TMPDIR under `target/review1-tmp/O`:

- `cargo test --locked`: every suite green. lib 210, `cp_tests` 27, `mv_tests` 15, `rm_tests` 19,
  crawl 25, find 9, version 6, `cli_version_tests` 1, s3 non-wire 4 (50 wire ignored), and
  `mv_xfs_tests` 3 ignored, as designed.
- `temp_index.sh` drive (release binaries from the export, confirmed with `command -v xdu-cp`):
  - `xdu-cp -u alice -n` listed 2 sources and destinations and wrote nothing (`stage` empty).
  - `--force -v`: `Copied: 2`, rc 0. A re-run gave `Existing at destination: 2`, rc 1.
  - `echo n | xdu-mv`: prompted `Move 1 file(s) into …? [y/N]` on stderr and printed `Aborted.`,
    rc 0, nothing written.
  - `xdu-mv --force -v`: `Moved: 1`; a re-run gave `Missing: 1`, rc 0.
  - `xdu-mv -i s3://b/p`: `S3 indices are not supported by xdu-mv`, rc 1.
- Each finding below was reproduced by O with its own probe script under
  `target/review1-tmp/O/probes/`: `xonly.sh`, `hive.sh`, `hive2.sh` and `blockround.sh`.
- `--help` against the man pages: every flag either tool's `--help` prints appears as `*--flag*` in
  its `.scd`. The page-only names are `xdu`'s `--apparent-size`/`--block-size`, `xdu-find`'s
  `--format`, and `--help`/`--version`.

**Reviewer A (ship)** and **reviewer B (block)**, each in its own export:

- `cargo test --locked` green (same counts as O). `cargo clippy --locked --all-targets
  --all-features -- -D warnings` and `cargo fmt --all -- --check` clean.
- R3: `s3://` through `-i` and `XDU_INDEX`, a missing `--into`, an `--into` that is a file, a path
  outside `--root` (naming the first offender, `-n` too) and source equal to destination. Every
  case exited rc 1 with empty stdout and the cause on stderr, and a whole-tree snapshot was
  unchanged.
- R4: an existing file, directory, symlink or dangling symlink at the destination name was left
  untouched, counted `Existing`, and gave rc 1. A symlinked intermediate directory under `--into`
  gave `destination component … is not a directory` and Failed, with nothing written through it.
- R4 interrupt: SIGKILL of `xdu-cp` mid-run, by A on 6 × 128 MB at 0.4 s and 1.0 s, and by B on
  8 × 64 MiB at 0.15, 0.4 and 0.8 s. No final name was ever partial; only `.xdu-*.part` temps
  remained, and every source was whole.
- R5 interrupt: B SIGKILLed a 3000-file `xdu-mv`. Source plus destination still held 3000, with no
  file at neither name. The re-run gave `Moved: 381 Missing: 2619`.
- R5 across filesystems, through the seams: `exdev_routes_to_the_copy_path`,
  `move_across_fails_closed_at_every_step`,
  `move_across_abandoned_at_every_step_leaves_the_file_whole_somewhere`,
  `move_across_flushes_file_and_directory_before_unlinking`, the grew, mtime-restored and swapped
  cases, `move_across_rolls_back_when_the_source_unlink_fails`, and
  `relocate::tests::the_production_policy_routes_exdev_to_the_copy_path`, all ok.
- R6: `y`, `yes` and `YES` proceed. `n`, `no`, `yep`, empty input and EOF print `Aborted.`, write
  nothing and exit 0. A dry run left the tree, the index, `--into`, HOME, the working directory and
  TMPDIR unchanged. `-l 5` selected identical rows in the dry run and the real run (`-j 8`), for
  both tools.
- R7: after a live mutation, each filter skipped the file for both tools: `--min-size`,
  `--max-size`, `--older-than`, `--newer-than`, `--mtime-older-than`, `--mtime-newer-than`,
  `--mode` (exact, `/`, `&`) and `--group`. A symlink swapped in gave `not a regular file`. A
  non-root caller cannot mutate the owner, so `--owner` rests on the unit boundary grid.
- R8: `xdu-find` and `xdu-cp -n` agreed on 26 filter sets (A) and 19 (B). The sets covered every
  filter, globs and regexes with `' [ ] * ? \`, `-u __root__`, and numeric and named `--owner`.
- R2 ownership: copying root-owned system files printed `copied (ownership not preserved)` and
  `Ownership not preserved: 3`, rc 0. Bytes, mode, uid, gid and nanosecond atime/mtime were
  carried.
- Real second filesystem: both reviewers' `hdiutil create` was refused by the sandbox ("Device not
  configured"), and every writable path is on one device. A ran the negative controls:
  `xfs-wire.sh "" 3` exits 1 with the variable unset and exits 1 on the same filesystem.

**Gates not observed locally**, all green in CI on `769255e` (`evidence/ci.md`, quoted from the run
logs):

- **S3 wire tier:** no pinned httpfs at the main checkout's `target/test-ext`. CI ran
  `s3-wire: '' 54 of 54 passed` on ubuntu-24.04 and macos-15.
- **Cross-filesystem tier:** the sandbox refused `hdiutil`. CI ran
  `xfs-wire: '' 3 of 3 passed` on `/dev/shm/xdu-xfs` (Linux) and on an APFS image (macOS), each
  after `test result: ok. 3 passed`. This closes the GOAL's 2026-11-10 cut checkpoint early, so
  R5's cross-filesystem path stands and no amendment is due.
- **Man-page render: unavailable here** (`scdoc` is not installed), so the published text of
  `xdu-cp.1` and `xdu-mv.1` was not read on this host. CI's packaging job printed
  `OK: every asserted literal survived into the published man-page text`. Its `check` lines
  assert `2x:XDU_INDEX`, `2x:XDU_JOBS`, `1x:s3://BUCKET/PREFIX` and each page's worked path once.
  Those are CI's count-form results, not a local read. **Unclosed gap for the human:** install
  `scdoc` (AGENTS.md, Commands) and read both pages.
- **`hpccm` `make check`:** `hpccm` is not installed here. The CI packaging job is green.
- **Linux leg** (`renameat2`, `faccessat`): this host is macOS. The CI `Test (ubuntu-24.04)` job is
  green.

**Red-gate hardening** (not product code): `spec/xdu-view-prefix-bytes-vs-chars/verify/red.sh`,
which reached this branch through the merge of `main`, was hardened after a stale-build false pass
(that cycle's META F3/F4). O probed the mechanism with a two-tree crate and one shared target dir:

- A tree carrying `git archive`'s old mtimes reused the previous mutant's library. It failed with
  the mutant's value while still printing `Compiling probe v0.1.0`.
- The same tree after red.sh's `find … -exec touch` stamp rebuilt and passed.
- So the stamp is sound and is the load-bearing fix. The `Compiling xdu v` guard cannot detect
  reuse, as that cycle's META F4 (open) already records.

## Requirement → evidence matrix

| R-ID | Implemented by | Verified how (by whom) | Status |
|------|----------------|------------------------|--------|
| R1 | `src/select.rs` (`select`, `selection_sql`, `checked_glob`), `src/relocate.rs`; `xdu-rm.rs`/`rm_tests.rs` diff empty | `rm_tests` 19/19; `select::tests::*`; partitions `q'x` and `'`/`]` select the right rows (A, B, O); the empty diff (O) | ⚠️ Hive-named partition or index path selects wrong rows (finding 3) |
| R2 | `relocate::copy_one`, `atomic_fs::copy_atomic` | `cp_tests` 27; metadata and ownership drives (A, B); smoke (O) | ⚠️ search-only ancestor fails every file (finding 4) |
| R3 | `relocate::run`, `check_whole_set`, `select::refuse_s3` | Whole-tree snapshot unchanged, rc 1 (A, B); `s3://` (O) | ✅ |
| R4 | `atomic_fs::copy_placed`, `rename_noreplace` | Collision matrix, SIGKILL at three points (A, B) | ✅ |
| R5 | `atomic_fs::move_same_fs`, `move_across`, `relocate::move_one` | SIGKILL of 3000-file mv (B); seam tests (A, B); CI `xfs-wire` 3/3 on both OSes (O) | ✅ (cross-fs real run: CI only) |
| R6 | `relocate::run`, `confirm` | Prompt inputs, dry-run snapshot, `-l` parity (A, B); smoke (O) | ✅ |
| R7 | `select::holds_live`, `live_sizes` | Every filter mutated, both tools (A, B) | ⚠️ `--max-size` fails open on a `--block-size` index (finding 2) |
| R8 | `cli::SelectArgs`, `select::build_filters` | `xdu-find` and `xdu-cp -n` set equality, 26 and 19 sets (A, B) | ✅ |
| R9 | `relocate::Summary`, `execute` | stdout/stderr split, rc on Existing/Failed only (A, B); smoke (O) | ✅ |
| R10 | `doc/xdu-cp.1.scd`, `doc/xdu-mv.1.scd`, packaging, README | R10 sentences, `--help`⊆`.scd`, completions, six-binary lists (A, B, O); CI literal step (O) | ✅ (render not read locally) |

Unmapped changes: the follow-up seeds `issues/www-cp-mv-at-release.md` and
`issues/xdu-mv-quarantine-unlink.md`, with their ROADMAP entries, and the hpccm smoke changing from
named binaries to a `bin/xdu*` loop (it serves R10; `check.sh` keeps the by-name check). None of
them changes behaviour, so none is scope creep.

## Findings

### 1. [HIGH/CONFIRMED] `invariants.md` §4 still covers only `xdu-rm`; the new destructive gates are absent
- **Where:** `.agents/factory/invariants.md:152` (§4, headed "`xdu-rm` destructive safety — read
  before touching `src/bin/xdu-rm.rs`").
- **Failure scenario:** the diff extends `AGENTS.md` invariant 4 (`AGENTS.md:537-539`) with
  "`xdu-cp` and `xdu-mv` carry the same gates … `xdu-mv` removes the source name, so it is
  destructive at the source". `invariants.md` says it is kept in lockstep with `AGENTS.md`, but its
  §4 never names either tool. A later diff that weakens `xdu-mv`'s prompt, its dry-run or its
  `--limit` order is therefore not an enumerated §1–§12 violation, so it is not auto-CRITICAL. Nor
  would `xdu-plan` walk §4 for a change to `relocate.rs` or `xdu-mv.rs`.
- **Evidence:** `git diff main...HEAD -- .agents/factory/invariants.md` has hunks only in the
  high-blast header (`@@ -25`), §5 (`@@ -162`) and §13. `sed -n '/^## 4\./,/^## 5\./p'` shows the
  `xdu-rm`-only text. (Both reviewers; O.)
- **Touches:** operating-manual drift that degrades a gate (rubric item 5, HIGH); invariant §4.

### 2. [MEDIUM/CONFIRMED] `--safe --max-size` acts on a file that outgrew the bound on a `--block-size` index
- **Where:** `src/select.rs:79-88` (`live_sizes`) and `:321-327` (`size_mode: None`). The claim
  is at `doc/xdu-cp.1.scd:148-157` and `doc/xdu-mv.1.scd:148-157`.
- **Failure scenario:** the index is built with `--block-size 1M`. A 900 KiB file, stored as
  1 MiB, is selected by `--max-size 1500K`. It then grows to 1.2 MB, which is 2 MiB by the index's
  own measure. `--safe` tests only the live length (1228800) and the disk usage. Both are under
  1.5M, so the file is copied or moved. The pages say the both-measures rule "fails closed", but
  for `--max-size` on a block-rounded index it fails open. The plan-gate ruling named two crawl
  measures, and the crawler has a third (`SizeMode::BlockRounded`).
- **Evidence:** `sh probes/blockround.sh` (O):
  - a fresh `--block-size 1M` index with `xdu-find --max-size 1500K` lists only `kept.dat`;
  - `xdu-cp --safe --max-size 1500K --force -v` on the first index printed
    `copied: …/u/grew.dat` and `Copied: 2`, rc 0.
  - A reproduced the same with `xdu-mv` (`Moved: 2`).
- **Touches:** R7; the published `--safe` claim. Nothing is lost (the file lands whole), but
  `--safe` admits a file its filter no longer selects. **Needs a foreman ruling:** narrow the man
  page claim, or record the size mode in the marker (an index-format change, §1).

### 3. [MEDIUM/CONFIRMED] A Hive-style `key=value` partition or index path rewrites a column, so the selection acts on the wrong rows
- **Where:** `src/select.rs:274`. `read_parquet(?)` leaves DuckDB's Hive-partition auto-detection
  on.
- **Failure scenario:** a top-level directory named `size=0` becomes a partition. With `-u size=0`
  every row's `size` reads 0, so `xdu-mv -u size=0 --max-size 1K --force` moves a 5,000,000-byte
  file. An index stored under any `<column>=<value>` directory does the same to every row. Under
  `…/mtime=0/index`, `--mtime-older-than 3650` selects files written seconds earlier, while the
  same tree indexed elsewhere gives `No matching files found.`. `--safe` catches the size and time
  cases, but without `--safe` the wrong set is acted on. The engine already refuses glob
  metacharacters for this same reason, so the same index path and partition name select rows
  other than their own.
- **Evidence:**
  - `sh probes/hive.sh` (O): `moved: …/size=0/big.dat`, `Moved: 1`, rc 0, and
    `ls -l arch/size=0` shows `5000000 big.dat`. The `mtime=0` index case listed `small.dat`, and
    the control index found nothing.
  - B reproduced both with its own scripts.
  - A mixed index with no `-u` selected correctly (`hive2.sh`), because auto-detection needs every
    path to carry the key.
- **Touches:** R1 ("selects the right rows"), R2/R5 ("exactly the matched set").
- **Pre-existing:** `xdu-find -u size=0 --max-size 1K` and `xdu-rm -u size=0 --max-size 1K -n`
  select the 5 MB file on this branch too (`hive2.sh`), and `AGENTS.md:22`'s "DuckDB derives no
  partition column from them" (2026-09-15) is false for such names. `xdu-rm` is a Non-goal here.
  Its share should be seeded (`issues/xdu-rm-on-shared-engine.md` or
  `issues/local-reader-bound-parameters.md`); it is not graded against this diff.

### 4. [MEDIUM/CONFIRMED] A source ancestor with search but not read permission fails every file beneath it
- **Where:** `src/atomic_fs.rs:92-103` (`open_dir_nofollow` opens each component
  `O_RDONLY|O_DIRECTORY|O_NOFOLLOW`), reached by `walk_nofollow` from `/`
  (`src/relocate.rs:243-251`, `:316-324`).
- **Failure scenario:** opening a directory read-only needs read permission on it. A 0711 (or
  0111) ancestor that `cp(1)`, `xdu-find` and `xdu-rm -n` all pass through makes `xdu-cp` and
  `xdu-mv` fail every file below it with `Permission denied`. The dry run previews N files and the
  real run fails all N. It fails closed: sources stay intact and the exit is rc 1. Root is
  unaffected.
- **Evidence:** `sh probes/xonly.sh` (O):
  - `cp(1) ok`;
  - `xdu-cp -n` gave `2 file(s) would be copied`;
  - `xdu-cp --force` gave `…/u/sub/a.dat: Permission denied (os error 13)`, `Copied: 0`,
    `Failed: 2`, rc 1, and `xdu-mv` gave the same;
  - after `chmod 755`, `Copied: 2`, rc 0.
  - B reproduced it on macOS. The same `open(2)` rule applies on Linux, where `O_PATH` (and
    `O_SEARCH` on macOS) would walk without read permission; not run on Linux here.
- **Touches:** R2/R5 ("exactly the matched set"). The man pages do not state the requirement.

**Dropped or routed after refutation:**

- **Routed to `META.md` F5:** three stale copies of the high-blast path list, in
  `review-rubric.md`, the skill's Safety Principles and `templates/REVIEW.md` (A, B). Rubric item
  5 scopes drift to `AGENTS.md` and `invariants.md`, which were both widened, and each copy defers
  to `invariants.md`, which this review applied.
- **Documented and seeded:** a rename onto the source name between the cross-filesystem re-check
  and the unlink is in `xdu-mv`(1) and seeded as `issues/xdu-mv-quarantine-unlink.md` (A, B).
- **Not reproduced:** the link route treating a pre-existing link to the same inode as a
  retransmitted success needs a race on top of the NFS fallback (A).
- **Dissolved under test (B):** brace globs (DuckDB treats them literally); a case-mismatched
  `--root`/`--into` (`realpath` corrects case); bound-regex speed (60k rows in 0.05 s).
- **Pre-existing, non-goals:** `xdu-find`'s quote error on `-u "q'x"` and its unordered `-l`.

## Human-gate triggers

- **Triggered.** Finding 1 touches the destructive invariant (§4). Findings 2 and 3 land in
  `src/select.rs`, and finding 4 lands in `src/atomic_fs.rs`. Both files are on `invariants.md`'s
  high-blast list, which is authoritative (the skill's and rubric's copies lag; see META F5).
- No CONFIRMED finding is in `src/cli.rs` or `src/lib.rs`, or against §2 (atomic write): the R4
  collision and SIGKILL probes held. **STOP for foreman sign-off before `/xdu-build`; nothing was
  remediated.** Finding 2 needs a ruling on the fix's shape.

## Optional completeness sub-pass

Skipped at the foreman's direction.

## Review cycle 2 — changes-requested (2026-11-07)

- **Reviewed commit:** `1ef271c` (pushed head; remediation `14b6170..1ef271c` on cycle 1's
  `dd79937`)  ·  **Base:** `main`  ·  **Date:** 2026-11-07
- **Verdict:** changes-requested (1 CONFIRMED: MEDIUM; 1 PLAUSIBLE: LOW). **Human gate
  triggered.**
- **Cycle:** 2 of ≤3. Cycle 3 is the last review the loop allows.

Mode: **debate, a fresh blind pass over the full spec-excluded diff against `main`**, the skill's
default for a later cycle. It was not scoped to the remediation. Two fresh blind reviewers ran in
parallel: A argued "ship" and B argued "block". Each got GOAL.md inline, in full this time, the
diff commands with `spec/` excluded, and its own `git archive` export of `1ef271c` without
`spec/`. The exports were refreshed in place over cycle 1's warm `target/`, and each blob was
checked against `1ef271c`. `review-rubric.md` and `invariants.md` were passed by path inside each
export, `cmp`-identical to HEAD's copies. Both reviewers were barred from `spec/`, cycle 1's
REVIEW.md included, and from other contexts' scratch. Both reported reading nothing there. Only
the orchestrator (O) read TECH.md's record of the foreman's cycle-1 gate rulings. Neither reviewer
was told the cycle-1 findings, though `git log --oneline main..HEAD` names them in the remediation
commit subjects (META F6). The completeness sub-pass was skipped at the foreman's direction.

Contract drift: none since cycle 1. `GOAL.md`'s only post-shape commits are still `edcc163` and
`3ce7375`, the foreman's approved rulings. No R-ID is a committed artifact under `spec/`.

### Cycle-1 findings: remediation (verified by O)

O re-ran cycle 1's probes (`target/review1-tmp/O/probes/`) against a fresh release build of
`1ef271c`. The export had its own `CARGO_TARGET_DIR` and TMPDIR, and the new probes are in
`target/review1-tmp/O/c2/probes/`.

| # | Finding | Re-run evidence | Status |
|---|---------|-----------------|--------|
| 1 | `invariants.md` §4 covers only `xdu-rm` | §4 is headed for all three tools. Its extracted text names `relocate::confirm`, `select::holds_live`, `deterministic_limit_clause` and "destructive at the source", and mentions `xdu-mv` 3 times. `AGENTS.md` invariant 4 is retitled the same way | ✅ remediated |
| 2 | `--safe --max-size` fails open on a `--block-size` index | `blockround3.sh` sets the file's mtime an hour back before the crawl, then grows it sparsely so that both live measures stay under 1500K. `xdu-cp` and `xdu-mv` print `skipped (changed since the index was built)`, `Skipped: 1`, and the file stays at its source. `blockround2.sh` (appended growth) skips by disk usage. Cycle 1's `blockround.sh` still copies the file, because it grows the file inside the crawl's second. That is the residual both man pages state (the index records mtime in whole seconds), not a regression | ✅ remediated per the ruling |
| 3 | Hive `key=value` names rewrite columns | `hive.sh`: `-u size=0 --max-size 1K` gives `No matching files found.` for `xdu-cp -n` and `xdu-mv --force`, and so does the index under `mtime=0/`. `hive2.sh`: the whole-index selection is 1 file, the correct one. `xdu-find` and `xdu-rm -n` still select the 5 MB file. That is pre-existing, outside this diff, and seeded as `issues/hive-partition-columns-find-rm.md` (high priority) | ✅ remediated |
| 4 | A search-only ancestor fails every file | `xonly.sh`: through a 0111 ancestor, `xdu-cp --force` gives `Copied: 2`, rc 0, and `xdu-mv --force` gives `Moved: 2`, rc 0 | ✅ remediated for ancestors (the directory holding the source: new finding 1) |

### Verification run

**O**, in its own export:

- `cargo test --locked`: 345 passed, 0 failed. The counts: lib 217, `cp_tests` 33,
  `mv_tests` 20, `rm_tests` 19, crawl 25, find 9, version 6, `cli_version_tests` 1, offline 1, view 1 + `xdu-view` 5,
  examples 4, s3 non-wire 4 (50 wire ignored), `mv_xfs_tests` 3 ignored. Every remediation test
  TECH.md names passed by name, the three registry and flush tests in `atomic_fs` included.
- `--help` against the man pages: each tool prints 23 long flags, and every one appears as
  `*--flag*` in its `.scd`. `src/cli.rs` is unchanged since cycle 1.
- Reproduced new finding 1 with B's `p5b_rename_control.sh` and O's own `mv_0311_source.sh`.

**Reviewers A (ship) and B (block)**, each in its own export:

- `cargo test --locked` gave the same counts as O. `cargo clippy --locked --all-targets
  --all-features -- -D warnings` and `cargo fmt --all -- --check` were clean.
- The CLI end to end: the R3 refusals over the whole set, naming the first offender in path order.
  `s3://` was refused through `-i` and through `XDU_INDEX`.
- R6: the prompt answers and a zero-write dry run, and `-l` parity between the dry run and the real
  run (A: `-l 3`; B: `-l 3` and `-l 4`).
- R7: every filter was mutated live, for both tools and for forced-EXDEV `xdu-mv`.
- R2: metadata carried to the nanosecond, and `Ownership not preserved` counted with rc 0.
- §2b: a missing marker and an unknown format are refused, and `errors=3` warns.
- §5: a `'` in both the index path and the partition selects the right rows. `{a,b}` and `]` are
  literal, and `i[ab]`, `-u 'z*'` and `-u ..` are refused.
- R4 collisions: an existing file, directory, symlink or dangling symlink at the final name is
  counted Existing and left untouched. A file or symlink at an intermediate component fails the
  file and is never followed. A name created mid-copy, during A's 1.5 GB copy, keeps the
  intruder's content, the temp is removed, and the run exits rc 1.
- Hive names: `size=0`, `mtime=0` and `uid=0` as a partition, in the index path, and in source
  paths all select the true rows.
- R5 interrupts:
  - Same filesystem: B SIGKILLed a 3000-file `xdu-mv` and A killed it twice mid-run. No file was
    at neither name.
  - Across filesystems: A and B each built an instrumented copy with a forced-EXDEV switch. A
    killed it 5 times on 24 × 24 MB plus 600 small files, and B 3 times. Each time no file was at
    neither name and no final name held a partial file.
  - A source written during the copy is skipped, with no destination left.
- **Durability, flush after `mkdirat`, and the `CreatedDirs` registry.** Both reviewers traced an
  instrumented build and checked that every publish and every source unlink follows a completed
  parent flush of each directory created on its path.
  - A ran 3,000 files under 333 new shared directories through cp, mv and forced-EXDEV mv, at
    `-j 64` and three times at `-j 256`. Each run made 8.7k–9.0k checks with 0 violations. The
    window was exercised: 360–1,737 pending-registry hits and 1–7 `EEXIST` races per run. The
    negative control, with the flushes stripped, flagged 8,787 of 8,787.
  - B: `p6_stress.sh mv 32 1500` made 2,425 directories and 12,000 checks with 0 violations.
    `p6_race.sh`, 64 workers racing on one new chain, ran 6 rounds with 0 violations.
  - Neither saw poisoning, deadlock, or an entry removed wrongly.
- Reviewer conduct: both left `git status --porcelain` empty. Both instrumented `src/atomic_fs.rs`
  and `src/relocate.rs` in their own export and restored them by re-extraction, verified by
  `git hash-object` and a rebuild. A's release `xdu-cp` and `xdu-mv` are `shasum`-identical to the
  pre-instrumentation build. B ran a full `cargo test` green after the restore. O then checked
  both exports blob by blob against `1ef271c`: 0 mismatches and no extra files. The instrumented
  binaries remain only under each reviewer's TMPDIR.

**Gates not observed locally:**

- The S3 wire tier: there is no pinned httpfs here.
- The cross-filesystem tier: the sandbox refuses `hdiutil`, so it was driven only through forced
  EXDEV, as above.
- The Linux arm (`O_PATH`, `renameat2`) on `1ef271c`: the foreman reports CI on `9c8cc32` green,
  that arm included. CI for `1ef271c` is running and the foreman owns the rollup, so it is **not
  observed** here.
- `hpccm` `make check`.
- Root-only paths: a successful chown to another uid, and a live `--owner` mutation.
- **The man-page render was unavailable here** (`scdoc` is not installed). The published text of
  `xdu-cp.1` and `xdu-mv.1`, including this remediation's new PERMISSIONS and SAFE MODE prose, was
  not read on this host. The `--help` flag sets were compared with the `.scd` sources by
  inspection (O, A, B). A and B counted the CI needles in the raw source, which is not CI's
  rendered count form. **Unclosed gap for the human:** install `scdoc` (`AGENTS.md`, Commands)
  and read both pages, or take the packaging job's literal step on `1ef271c`.

### Requirement → evidence matrix (cycle 2)

| R-ID | Verified how (by whom) | Status |
|------|------------------------|--------|
| R1 | `rm_tests` 19/19 and the `xdu-rm.rs`/`rm_tests.rs` blobs unchanged (A, B); `'` in the index and the partition; glob refusals; Hive off (A, B, O) | ✅ |
| R2 | Layout, metadata and ownership drives; non-regular sources skipped (A, B) | ✅ |
| R3 | Whole-set refusals with nothing written; `s3://` (A, B) | ✅ |
| R4 | Collision matrix; mid-copy intruder; SIGKILL leaves no partial final name (A, B) | ✅ |
| R5 | SIGKILL same-fs and forced-EXDEV; source written during the copy; flush trace (A, B); seam tests (O) | ⚠️ the same-fs rename is refused for a source directory, or an existing destination directory, that the user cannot read (finding 1). Real cross-fs run: CI only |
| R6 | Prompt answers, zero-write dry run, `-l` parity (A, B) | ✅ |
| R7 | Every filter mutated live, both tools and forced EXDEV (A, B); the block-size rule (O) | ✅ |
| R8 | Flag sets against `xdu-find`; `*_accepts_every_xdu_find_filter` (A, B) | ✅ |
| R9 | Separate counts; exit status; stdout and stderr split (A, B) | ✅ |
| R10 | R10 sentences, `--help` ⊆ `.scd`, completions, six-binary lockstep (A, B, O) | ✅ (render not read locally) |

Unmapped changes: `issues/hive-partition-columns-find-rm.md` and its ROADMAP entry, the seed the
cycle-1 ruling required. No scope creep.

### Findings (cycle 2)

#### 1. [MEDIUM/CONFIRMED] `xdu-mv` refuses a same-filesystem rename when it cannot read the source directory or an existing destination directory
- **Where:**
  - `src/relocate.rs:343-345`: `move_one` walks to the source directory with `Access::Read`.
  - `src/relocate.rs:354-362`: its `dest` closure walks to the destination directory with
    `Access::Read`.
  - Both run before the rename, on every route.
- **Failure scenario:** a non-root user is on one filesystem. The source directory is 0311
  (write and search, no read), or an existing destination directory is a 0333 drop box. `xdu-mv`
  fails the file with "cannot open directory s for reading (Permission denied), which flushing it
  to stable storage needs", and exits 1. `mv -n` moves the same file.
  - The message is false on this route: `move_same_fs` flushes neither directory. Only a created
    directory's parent is flushed, and only after `EXDEV` does `move_across` flush the source
    directory and the copy's destination directory.
  - The foreman's cycle-1 ruling requires a read open where a directory **is flushed**. This
    applies it to directories the same-filesystem route never flushes. R5's "On one filesystem it
    SHALL rename" is refused, though it fails closed with the source intact.
  - `xdu-cp` into a 0333 destination directory fails too. That one is within the ruling: the copy
    flushes that directory after its publish.
- **Evidence:**
  - `sh target/review1-tmp/O/c2/probes/b_p5b_rename_control.sh` (B's probe, O's build) gives
    `Moved: 0`, `Failed: 2`. `mv -n` then moves both files.
  - `mv_0311_source.sh` gives `xdu-mv rc=1` with `Failed: 1`, while `xdu-cp` copies the same file
    (`Copied: 1`, rc 0) and `mv(1)` moves it.
  - B reproduced it independently (`p5_searchonly.sh`, `p5b_rename_control.sh`). A saw the same
    behaviour and dropped it as documented (`doc/xdu-mv.1.scd:262-278`).
  - `mv_fails_closed_on_a_source_directory_it_cannot_read` pins the current behaviour on the
    plain, same-filesystem route.
- **One remediation that closes it:**
  - In `move_one`, walk the source directory and the destination directory search-only for the
    no-replace rename.
  - Only on the `CrossFs::Copy` route, after `EXDEV`, reopen each for reading. `DirHandle::reopen`
    opens `.` from the held handle, or the walk can be repeated with `Access::Read`. Keep today's
    fail-closed message there. The per-worker `ParentCache` must not hand a search-only handle to
    that route.
  - Created directories keep their parent flush.
  - Move the pinning test to the forced-EXDEV seam. Add a red-then-green `mv_tests` case that
    moves out of a 0311 source directory and into an existing 0333 destination directory on one
    filesystem.
  - Narrow `xdu-mv`(1)'s PERMISSIONS sentence: read on those two directories is needed only for a
    move across filesystems.
  - Alternatively the foreman may rule today's documented behaviour acceptable at the gate.
- **Touches:** R5. The file is `src/relocate.rs`, on the high-blast list. It concerns `xdu-mv`'s
  §4 gate, but no §4 or §2 rule is broken: nothing is lost and the exit is non-zero.

#### 2. [LOW/PLAUSIBLE] The created-directory registry covers only the current run
- **Where:** `src/relocate.rs:483` builds a fresh `CreatedDirs` for each run, and
  `src/atomic_fs.rs:354-361` flushes on a plain open only when the path is pending.
- **Failure scenario:**
  1. Run 1 is killed after a `mkdirat` and before that directory's parent flush. The same happens
     with a directory another process created.
  2. Run 2 opens the directory with no flush, moves a file into it across filesystems, flushes the
     file and the directory, and removes the source.
  3. Power is lost before the parent's entry reaches disk, on a filesystem where flushing a child
     does not order its parent's entry.
  4. The file is then at neither name.
- **Evidence:** reading only (B). It needs a crash, and was not reproduced. ext4 and XFS journal
  ordering covers it.
- **For human triage only; it does not drive the verdict.**

**Dropped or routed after refutation:**

- **Routed to `META.md` F5 (· seen again):** the three restated copies of the high-blast path list
  in `review-rubric.md`, the skill's Safety Principles and `templates/REVIEW.md` still lack the
  four new paths. A graded it MEDIUM and B HIGH. Cycle 1 routed it the same way. Rubric item 5
  scopes drift to `AGENTS.md` and `invariants.md`, both current. The skill and the template defer
  to `invariants.md`, and the foreman's remediation plan left F5 to `/xdu-harness`.
- **Dropped:**
  - Case-variant `CreatedDirs` keys (A, B): these need a case-insensitive destination and a power
    loss. Speculative.
  - A losing worker's non-`EEXIST` `mkdirat` error clearing another worker's pending entry (B): no
    reachable errno does it.
  - `--safe`'s mtime rule skipping touched but unchanged files (A): this is the ruled behaviour,
    and it is documented.
- **Documented and seeded:**
  - The window between the re-check and the unlink (`issues/xdu-mv-quarantine-unlink.md`).
  - EPIPE under `-v | head` (`issues/broken-pipe-closed-stdout.md`).

### Human-gate triggers (cycle 2)

- **Triggered.** Finding 1 is CONFIRMED in `src/relocate.rs`, which is on `invariants.md`'s
  authoritative high-blast list, and it concerns `xdu-mv`'s destructive gates (§4).
- No CONFIRMED finding in `src/select.rs` or `src/atomic_fs.rs`, and none against §2: the
  flush-order traces, the registry stress runs and every SIGKILL probe held.
- **STOP for foreman sign-off before `/xdu-build`; nothing was remediated.** Cycle 3 is the last
  review the loop allows, so the foreman's choice for finding 1 is to remediate it as described
  above, or to rule the documented behaviour acceptable.
