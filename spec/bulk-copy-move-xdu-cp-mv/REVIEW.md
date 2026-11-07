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
