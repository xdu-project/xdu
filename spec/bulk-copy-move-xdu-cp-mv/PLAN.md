# PLAN — Bulk copy and move: `xdu-cp` and `xdu-mv` over a shared select-act engine

> **Status:** Draft for review · **Last updated:** 2026-11-06
> **Authoritative technical design.** The *how*. Vision/contract is [`GOAL.md`](GOAL.md);
> the phased executable roadmap is [`TECH.md`](TECH.md). Backing detail is in
> [`research/`](research/) (digest: [`research/00-digest.md`](research/00-digest.md)). Every design
> element traces to a GOAL R-ID.

## 1. Summary

Two new thin binaries, `xdu-cp` and `xdu-mv`, over three new library modules: `src/select.rs`
(the selection: gates, a bound-parameter statement always ordered by path, the `--safe` live
predicate; no rayon, no destination, so `xdu-tar` and a later `xdu-rm` can take it alone),
`src/atomic_fs.rs` (the filesystem primitives: symlink-safe directory walks, a fail-closed
no-replace rename, an atomic and durable copy, the cross-filesystem move) and `src/relocate.rs`
(the `--root` path mapping, the whole-set refusals, dry-run, confirmation, the ordered parallel
executor and the counted summary). `xdu-rm` and its tests are untouched. The phases land the
publishable core first: `xdu-cp` (P1, P2), `xdu-mv` on one filesystem (P3), documentation and
packaging (P4), then `xdu-mv` across filesystems (P5). That puts the GOAL's first cut on the last
phase, as its Notes ask, at the price of running documentation before the cross-filesystem work
instead of after it (the adversarial review, §4, showed the other order leaves packaging undone
on the cut checkpoint's day). P5 amends `xdu-mv(1)` and re-counts its literals.

## 2. Design

### 2.1 Module boundaries

| File | Role | Blast |
|------|------|-------|
| `src/select.rs` (new) | `SelectArgs` → `Selection { index, filters, warning, rows }`; `selection_sql`; `checked_glob`; `Live`, `Verdict`, `holds_live` | high (new destructive-path selection) |
| `src/atomic_fs.rs` (new) | `DirHandle`, `open_dir_nofollow`, `walk_create_nofollow`, `open_source`, `rename_noreplace`, `copy_atomic`, `move_across`, `sync_durable`; the crate-private fault seam | high (the only code that unlinks a source) |
| `src/relocate.rs` (new) | `RootMap`, `check_whole_set`, `confirm`, `Executor`, `Outcome`, `Summary`, `run_cp`, `run_mv` | high |
| `src/lib.rs` | `pub mod select; pub mod atomic_fs; pub mod relocate;` only; `QueryFilters` is reused as it stands | high-blast file, three lines |
| `src/cli.rs` | `SelectArgs` (`#[derive(Args)]`), `XduCpArgs`, `XduMvArgs` (`#[command(flatten)]`), each with `version` | high-blast file, additive only |
| `src/bin/xdu-cp.rs`, `src/bin/xdu-mv.rs` (new) | parse args, call `relocate::run_cp`/`run_mv`, map the result to an exit status | thin (§11) |
| `src/bin/gen-completions.rs`, `Cargo.toml` | two entries each | low |
| `tests/cp_tests.rs`, `tests/mv_tests.rs`, `tests/mv_xfs_tests.rs`, `tests/xfs-wire.sh` (new); `tests/common/mod.rs` (two `binary_path` arms, `run_cp`, `run_mv`, `run_with_stdin`; nothing existing changes) | tests | low |

`src/bin/xdu-rm.rs`, `tests/rm_tests.rs`, `XduRmArgs`, `XduFindArgs`, `parquet_source`,
`deterministic_limit_clause` and `QueryFilters::to_conditions`'s output are unchanged; every
phase gate checks the first two with `git diff --quiet edcc163 -- …`.

### 2.2 Selection (`src/select.rs`, R1, R3, R6, R8)

Order inside `select()`; the binary runs the S3 refusal before it:

1. **S3 refusal** on the raw `OsStr` (`s3::is_s3_locator`), before the pool, `canonicalize`, the
   extension directory or any query: `xdu-cp: S3 indices are not supported by xdu-cp`, exit 1
   (R3, as `xdu-rm` does).
2. `canonicalize` the index; `index_version_error` → refuse; `index_completion_warning` →
   returned for the caller to print on stderr (§2b, R1).
3. `build_filters`: `resolve_user`/`resolve_group` on the main thread, then the same
   `QueryFilters` builder chain as `xdu-find` (R8).
4. `checked_glob(index, partition)`: refuse a non-UTF-8 index path or one containing `*` `?` `[`
   `\`; refuse a partition name that is empty, `.`, `..`, holds `/` or one of those characters;
   refuse a `-u` naming no directory under the index ("no partition NAME in INDEX"). Then
   `index_glob(index, partition)` (§3: the one place the layout becomes a glob). Research 03
   probed why: a bound string is still a glob, and `idx[ab]` silently reads a sibling `idxa`.
5. `selection_sql(&filters, limit) -> (String, Vec<String>)`, pure: `SELECT path, size, uid,
   gid, mode, atime, mtime FROM read_parquet(?) [WHERE …] <tail>`. Bound values: the glob, then
   the regex (`regexp_matches(path, ?)`) when a pattern is set. Every other predicate comes from
   calling the existing `QueryFilters::to_conditions` on a clone whose `pattern` is cleared, so the
   predicates exist once and `src/lib.rs` needs no refactor. The tail is
   `deterministic_limit_clause(limit)` when a limit is set (it already reads `ORDER BY path
   LIMIT n`) and the literal `ORDER BY path` otherwise, so the order is unconditional (research
   03: 0.2 s over 5 M rows) and never written twice.
6. `engine::open_connection()`, `prepare`, `query(params_from_iter(&params))`, collect `Vec<Row>`.

`select.rs` imports neither rayon nor anything from `atomic_fs`/`relocate`; a gate greps that.

**`--safe` predicate (R7).** `Live` is built from an `fstat` of the opened, `O_NOFOLLOW` source
fd (cp, cross-filesystem mv) or an `fstatat(parent, name, AT_SYMLINK_NOFOLLOW)` (same-filesystem
mv): both are `lstat` semantics on the very object acted on. `holds_live(&QueryFilters, &Live)
-> Verdict { Holds, NotRegular, Changed(flag) }` mirrors `to_conditions` against the epochs
`QueryFilters` stored at build time (never a fresh `now`): size, atime, mtime, uid, gid, mode
(`Exact`/`Any`/`All`). The pattern holds by construction. **Size** needs a measure, because the
index's size mode (disk usage by default, apparent, or block-rounded) is not recorded. `Live`
carries the row's indexed `size`, and the check picks per row: if the live length equals the
indexed size, the filter is tested on the length; else if the live disk usage (`blocks*512`)
equals it, on the disk usage; otherwise (the file changed since the crawl, or a block-rounded
index) the filter must hold for **both** measures. An unchanged file therefore always holds, and
a changed one fails closed: a file selected by `--min-size 2K` that shrank to 100 bytes is
skipped whatever its block count. This is the plan's reading of R7's "size against
`--min-size`/`--max-size`"; it is put to the foreman for a GOAL Clarification (§5), and the
man pages state it.

### 2.3 Path mapping and whole-set refusals (`src/relocate.rs`, R2, R3)

- `--into` must exist and be a directory (`metadata`, which follows an operator-named symlink;
  `--into` is then canonicalized once). Checked before `select()`, so a bad `--into` refuses
  before any query (R3).
- `--root` defaults to `/`; it is canonicalized (index rows hold the canonical crawl root, so on
  macOS `/var/…` must become `/private/var/…`). A missing `--root` refuses.
- `RootMap::map(row) -> Result<(rel, dest), Refusal>`: the row path must be absolute and
  normalized (no empty, `.` or `..` component; a hand-edited index cannot climb out), and
  `Path::strip_prefix(root)` must succeed component-wise (`/data2` is not under `/data`). The
  destination is `into.join(rel)`. A destination equal to its source path refuses.
- `check_whole_set` maps every row in `ORDER BY path` order before the first write, dry-run
  included, and refuses naming the first offending path (R3). The set is mapped once and the
  `(src, rel)` pairs are what the executor consumes.

Aliasing that lexical comparison cannot see (a bind mount, a symlinked `--into` resolving into the
source tree) is still safe: the destination name already exists, so the no-replace publish counts
it as existing and nothing is replaced.

### 2.4 Gates, dry-run and confirmation (R6)

After the whole-set check: an empty selection prints `No matching files found.` on stdout and
exits 0. `--dry-run` prints `SRC -> DEST` per row in path order and `N file(s) would be copied
into INTO` (or `moved`) on stdout, then exits 0 having made no filesystem write: no `mkdirat`, no
temp, no unlink; it does not open `--into` for writing. Otherwise, without `--force`, the prompt
`Copy N file(s) into INTO? [y/N] ` goes to **stderr** (R9 keeps stdout for the listing and
summary; `cp -i` prompts on stderr); only `y`/`yes` (case-insensitive, trimmed) proceeds;
anything else, EOF included, prints `Aborted.` on stderr and exits 0 with nothing written, as
`xdu-rm` does. Because selection is always `ORDER BY path` and `LIMIT` comes from the same
builder, `--dry-run` and the real run select identical rows (§4).

### 2.5 Filesystem primitives (`src/atomic_fs.rs`, R2, R4, R5)

All calls through `libc` 0.2.191, already a runtime dependency (research 01). `#[cfg(target_os =
"linux")]` and `#[cfg(target_os = "macos")]` arms; any other Unix takes the `linkat` route only.

- **Walks.** `DirHandle(OwnedFd)`. Destination: from the canonical `--into` fd, each component
  `openat(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`; on `ENOENT`, `mkdirat(0o777)` (umask applies;
  `EEXIST` from a racing worker is success) and reopen; `ELOOP`/`ENOTDIR` fails the file
  ("destination component is not a directory"). Source: from `/`, the same descent without
  creating, then `fstatat(parent, name, AT_SYMLINK_NOFOLLOW)`: not `S_ISREG` (a symlink, a
  directory, a FIFO, a device) → skipped, never opened, so no device driver sees an open (R2's
  regular-file rule, §8); `ENOENT` anywhere → missing. Only then `openat(parent, name,
  O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC)` and `fstat`, whose (dev, ino) must equal the
  `fstatat`'s, or the file is skipped as changed. Each worker caches its last source and destination parent
  `DirHandle` keyed by the relative directory, so ordered input costs about one `openat` per
  directory.
- **`rename_noreplace(sp, sn, dp, dn) -> Result<(), NoReplace>`** where `NoReplace = Existing |
  CrossDevice | Unsupported | Io(io::Error)`:
  1. Linux `syscall(SYS_renameat2, sp, sn, dp, dn, RENAME_NOREPLACE)` (no glibc symbol); macOS
     `renameatx_np(…, RENAME_EXCL)`.
  2. `EEXIST` → `Existing`; `EXDEV` → `CrossDevice`.
  3. `EINVAL`, `ENOSYS`, `ENOTSUP`, `EOPNOTSUPP` → the link route. On NFS (the usual HPC
     destination) this is the main path, so it carries its own identity checks. Before the
     call, record the source's (dev, ino) from `fstatat(sp, sn, NOFOLLOW)`; the caller has
     already established that nothing exists at `dn` (a destination that is already a hard link
     to the source is therefore `Existing`, never "moved"). `linkat(sp, sn, dp, dn, 0)`. Success
     → `sync_durable(dp)` → re-`fstatat(sp, sn, NOFOLLOW)`: its (dev, ino) must still equal the
     recorded pair and the new link's, else the source name was swapped in the window, so remove
     the link we made (inode-checked) and fail the file → `unlinkat(sp, sn)`. `linkat` `EEXIST` →
     `fstatat(dp, dn, NOFOLLOW)`; the recorded (dev, ino) means an NFS retransmit succeeded,
     otherwise `Existing`. `linkat` `EXDEV` → `CrossDevice`. `EPERM`, `ENOTSUP`, `EMLINK` →
     `Unsupported` ("destination supports neither a no-replace rename nor hard links").
     `unlinkat` `ENOENT` after a verified link → ok.
  4. Any other errno → `Io`. No code path calls `rename(2)`, `std::fs::rename` or
     `std::fs::copy`; a phase gate greps the new files for them.
- **`sync_durable(fd)`**: macOS `fcntl(F_FULLFSYNC)`, falling back to `fsync` on `ENOTSUP`,
  `EINVAL` or `ENOTTY`; Linux `fsync`. Used for files and directories (directory `fsync` and
  `F_FULLFSYNC` probed to return 0 on APFS).
- **`copy_atomic(src: &SourceFile, dest: &DirHandle, name, faults) -> Result<Placed, Failure>`**,
  where `SourceFile` carries the fd and its S0 `fstat`:
  1. `fstatat(dest, name, NOFOLLOW)`: anything there → `Placed::Existing` (no bytes read; also
     makes the count OS-independent, research 02).
  2. Temp `.xdu-<pid>-<seq>-<rand>.part` via `openat(O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC,
     0o600)`; it never holds part of the final name.
  3. Copy bytes (`std::io::copy` from the source fd).
  4. `fchown(uid, gid)` from S0. Any error from it is a refusal (`EPERM`, `EINVAL` from an NFSv4
     idmap or an unmapped id, `EACCES`, `ENOTSUP`): retry `fchown(-1, gid)` so the group can
     survive a refused owner, then count ownership not preserved; never a failure (R2). When
     either id was not carried, the mode below drops `S_ISUID|S_ISGID`, as GNU `cp -p` does.
     `fchmod(mode & 0o7777)` second, because `chown` clears the set-id bits.
     `futimens([S0.atime, S0.mtime])` with nanoseconds last. S0 is taken before any read, so the
     carried atime is the pre-copy one.
  5. `sync_durable(temp)`. Mandatory for both tools: without it XFS or ext4 can expose a
     zero-length final name after power loss, which breaks R4.
  6. S1 = `fstat(src fd)`, **for both tools**: size, mtime (s, ns) or ctime (s, ns) differing
     from S0 means the source was written during the copy, so the temp holds torn bytes. Unlink
     the temp and count the file skipped (changed during copy). A torn copy stamped with S0's
     mtime would otherwise look unchanged to any later size-and-mtime comparison.
  7. `rename_noreplace(dest, temp, dest, name)`; `Existing` → unlink the temp, `Placed::Existing`.
  8. `sync_durable(dest)`.
  On any error after step 2 the temp is unlinked before returning; a crash leaves at most a
  `.xdu-*.part` file, never a partial file at a final name (R4). The man pages say how to find
  leftovers.
- **Same-filesystem move**: **always** (not only under `--safe`) `fstatat(src_parent, name,
  NOFOLLOW)` first; anything but `S_ISREG` (a directory would carry its whole subtree, a FIFO or
  symlink would move too) is skipped and counted (R5 takes R2's regular-file rule; §8). With
  `--safe`, `holds_live` runs on that same `fstatat`. Then the existing-destination pre-check and
  `rename_noreplace(src_parent, name, dest_parent, name)`. The inode, owner, mode and times
  travel with it. An object swapped in between the `fstatat` and the rename is moved as itself,
  never followed (renames do not follow a final symlink); documented in `xdu-mv(1)`.
- **`move_across` (cross-filesystem, P5)**, taken only when the same-filesystem rename returns
  `CrossDevice`:
  1. `open_source` → fd, S0. `--safe` check on S0. Pre-check that the source parent is
     writable and searchable (`faccessat(W_OK|X_OK, AT_EACCESS)`); if not, fail the file before
     copying, because the final unlink would fail.
  2. `copy_atomic` steps 1–6 (including the S1 check: a source written during the copy leaves
     nothing visible and is skipped).
  3. Publish (step 7), then `sync_durable(dest)` (step 8). A flush failure here unlinks the
     final name we created (only after `fstatat` shows the temp's inode) and fails the file;
     the source stays.
  4. S2 = `fstatat(src_parent, name, NOFOLLOW)`. dev, ino, type, size, mtime or ctime differ
     from S0 → unlink our destination (inode-checked), keep the source, skipped.
  5. `unlinkat(src_parent, name)`. If it fails (`EACCES`, `EPERM` in a sticky directory,
     `EROFS`, anything), unlink our destination (inode-checked) and count the file failed with
     the errno on stderr: the operator asked for a move, and a copy that silently stays beside
     its source would read as `Existing` on a re-run. Then `sync_durable(src_parent)`
     (best-effort: a lost unlink only restores "both").
  The unlink is issued only after the file and directory flushes returned, so a crash between any
  two steps leaves the file whole at the source, the destination, or both (R5). Remaining races
  (a write between S2 and `unlinkat`, a writer holding the file open, `mmap` writers, a same-size
  rewrite inside one clock tick, a swap of the final source name inside its directory) are
  documented in `xdu-mv(1)`; a quarantine rename that would close the last one is noted in §5
  as a possible hardening, not built.
- **Fault seam.** `copy_atomic` and `move_across` take a crate-private `&mut dyn StepHook`
  (`fn at(&mut self, Step) -> HookAction { Continue, Fail(io::Error), Abandon }`); production
  passes `NoHook`. `rename_noreplace` and `fchown` have `Faults` overrides (force the `linkat`
  route, force `EPERM`, force `EXDEV`). Unit tests in `atomic_fs` reach them; nothing public
  exposes them.

### 2.6 Executor, outcomes and output (R1, R9)

A local `rayon::ThreadPoolBuilder` pool of `-j` threads (`XDU_JOBS`), never `build_global`, so
`xdu-tar` and tests are not fighting the crawler's global pool (§7 is the crawler's shape and is
untouched). Rows are processed in ordered chunks of 1024: `chunk.par_iter().map(act).collect()`
keeps index order; the main thread prints and tallies each chunk in order, so output is
deterministic and streams.

`Outcome = Done { owner_kept } | Missing | Existing | Skipped(reason) | Failed(msg)`.
`Summary` counts copied/moved, missing, existing at destination, skipped (`--safe` changed,
not a regular file, changed during copy), ownership not preserved, failed (R9).

- stdout: the `-v` per-file listing (`copied: SRC -> DEST`, `missing: SRC`, …), the dry-run
  listing, the empty-set message and the summary (one line per non-zero count; the action count
  always: `Copied: N` / `Moved: N`). Same shape as `xdu-rm`'s summary.
- stderr: the completion warning, the prompt, `Aborted.`, every failure line (always, not only
  under `-v`), and every refusal.
- Exit: 1 when `failed > 0 || existing > 0`, after the summary; non-zero for every refusal
  before any write; 0 otherwise, including runs whose files were all missing, skipped or
  ownership-not-preserved.

### 2.7 CLI (`src/cli.rs`, R8, §10)

`SelectArgs` holds, in `xdu-find`'s declaration order and wording: `-i/--index` (`XDU_INDEX`),
`-p/--pattern`, `--regex`, `-u/--partition`, `--min-size`, `--max-size`, `--older-than`,
`--newer-than`, `--owner`, `--group`, `--mode`, `--mtime-older-than`, `--mtime-newer-than`, then
`xdu-rm`'s `-l/--limit`, `-n/--dry-run`, `--safe`, `-f/--force`, `-v/--verbose`, `-j/--jobs`
(`XDU_JOBS`, default 4). `XduCpArgs`/`XduMvArgs` add `--into DIR` (required) and `--root DIR`
(default `/`), each with `name`, `version` and `after_help` examples. Research 03 probed that
flatten gives byte-identical help and completions. `-f` is `--force` here as in `xdu-rm`, not
`xdu-find`'s `--format`; the man pages say so.

### 2.8 Man pages, README, packaging (R10, §10, §13)

`doc/xdu-cp.1.scd` lands with the CLI in P1, `doc/xdu-mv.1.scd` in P3 (§10 same-commit rule);
P2 and P5 amend them. Both carry the two R10 sentences verbatim, so a gate can find them:
"The index is not updated." and "Reading a source may update its atime" (continuing: under the
mount's options, which can move it out of a later `--older-than` selection or fail a later
`--safe` re-check against an older index). Both also state the size reading of `--safe`, the
glob-metacharacter refusal, `.xdu-*.part` leftovers, and each page's own worked mapping example
with distinct paths. P4 does the lockstep set (research 04): `release.yaml` (glibc/allow-list,
Linux and macOS assembly loops), `packaging/extensions/check.sh` (three loops) with the
`Dockerfile` in the same commit, `install.sh` `BINARIES`, `hpccm/xdu.py` smoke as a glob over
`bin/xdu*` (version-independent against the pinned 0.6.0 tarball) with regenerated
`xdu.def`/`xdu.docker`, `test.yaml`'s `check` lines and produced-file list, `cli_version_tests`,
the rpm spec, `AGENTS.md` and `invariants.md` counts and lists, `issues/manpage-gate-coverage-gaps.md`
and its ROADMAP paragraph, and `README.md` with one copy and one move. `doc/xdu-rm.1.scd` is not
touched; `www/` is not touched (it deploys on push to `main` and would advertise unreleased
binaries; the deferral is filed by P4 and walked by the last phase's ledger).

### Requirement → design map

| R-ID | Design element(s) that satisfy it |
|------|-----------------------------------|
| R1 | `select.rs` (gates §2b, filters, `ORDER BY path`, bound glob and regex, `checked_glob`, no rayon), `relocate.rs` (dry-run, confirm, `--force`, executor, summary) shared by both bins; `holds_live`; `xdu-rm.rs`/`rm_tests.rs` unchanged (gate) |
| R2 | `RootMap`; `walk_create_nofollow`; `copy_atomic` (bytes, `fchown` best-effort counted, `fchmod`, `futimens` ns); `open_source` regular-file rule; cp never writes through a source fd |
| R3 | S3 refusal first; `--into` check before `select()`; `check_whole_set` over the ordered set before the first write, dry-run included |
| R4 | existing pre-check + `rename_noreplace` publish; temp `.part` never at a final name; temp flushed before publish; unlink-on-error |
| R5 | same-fs `rename_noreplace`; `CrossDevice` → `move_across` (S0/S1/S2, flushes before unlink, inode-checked rollback); fault-seam unit tests; `XDU_TEST_XFS_DIR` integration tier in CI |
| R6 | §2.4: prompt naming count and `--into`, `--force`, zero-write dry-run, empty-set exit 0, identical rows under `--limit` |
| R7 | `holds_live` over `Live` from `fstat`/`fstatat` NOFOLLOW, every filter, size by the measure that matches the indexed size (both when neither does); DuckDB boundary-grid test |
| R8 | `SelectArgs` flattened into both arg structs |
| R9 | `Outcome`/`Summary`, stream split, exit rule |
| R10 | two `.scd` pages, completions, the P4 lockstep set, README examples |

## 3. Invariant gate (AGENTS.md constitution check)

Gate #1 (before research) listed §2, §2b, §4, §5, §6, §7, §8, §10, §11, §13 and found the GOAL
sane against all of them: §4 is written for `xdu-rm` and the GOAL adopts it whole for the new
tools; §5's raw-interpolation exception is what R1 removes for the engine; nothing touches §1's
schema. Gate #2 (after this design):

- **§1 schema** — read-only use of the eight columns (`ctime` unused); no format change.
- **§2 atomic writes** — §2 governs index chunks, but its rule carries over: every copy is
  written under a temp name in the **final directory** and published by a no-replace rename or
  link in that same directory; a final name never holds a partial file (R4). The temp name is a
  dotfile with a fixed suffix, like `.partial`.
- **§2b gates** — `index_version_error` refuses before `index_completion_warning` warns, both
  before the selection; the S3 refusal precedes both.
- **§4 destructive safety** — prompt (`y`/`yes` only), zero-write `--dry-run`, `--force`, empty
  set exits 0; `ORDER BY path` always, so `--limit` is deterministic; `--safe` re-checks every
  filter (closing, for the new tools, the gap §4 records for `xdu-rm`). `xdu-mv` destroys at the
  source only after the copy and its directory are durable and S2 matches S0.
- **§5 injection** — the glob and the regex are bound; numerics are typed; glob
  metacharacters and path-climbing partition names are refused before SQL. `parquet_source`'s
  local text is unchanged (its byte-for-byte pin holds). P4 updates §5 and `AGENTS.md` to name
  the engine's bound statement.
- **§6 Unix-only** — `libc` and `MetadataExt` throughout; Linux and macOS arms, link-only
  elsewhere.
- **§7 rayon** — the crawler's global pool is untouched; the new tools build a local pool.
- **§8 regular files** — `O_NOFOLLOW` + `S_ISREG` on the fd; symlinks and specials skipped and
  counted.
- **§10 CLI** — one definition in `src/cli.rs`; each CLI change ships its `.scd` in the same
  commit; completions from `gen-completions`.
- **§11 thin binaries** — the bins parse and call `relocate::run_*`; all logic is in the library.
- **§13 conventions** — version from `Cargo.toml` (`version` on both new `#[command]`s), stdout
  clean, packaging lockstep in one phase, no `spec/` ids in source, counted man-page literals.

### Deviation justifications

| Deviation | Why needed | Simpler alternative rejected because |
|-----------|-----------|--------------------------------------|
| The engine refuses an index path or partition name containing glob metacharacters (`*` `?` `[` `\`), `/` or `..`, which `xdu-find` still reads | A bound glob is still a glob: probes selected a sibling index or another index with no error (research 03). A copy or move must never act on a set the operator did not name | Escaping each metacharacter as a one-character class works in DuckDB 1.4.4 but is undocumented; a DuckDB bump could change it silently. Left to `issues/local-reader-bound-parameters.md`, where the readers move with it |
| Symlink-safe component walks on both source and destination (more code than `create_dir_all` + path ops) | A symlink planted in a shared destination tree would redirect a root run's writes; a source parent swapped for a symlink after indexing would make a root `xdu-mv` move a file outside the indexed tree. No-replace stops overwrites, not misdirected creation or removal | Path-based `std::fs` follows every intermediate symlink; `O_NOFOLLOW_ANY`/`openat2(RESOLVE_NO_SYMLINKS)` are not on both platforms at the glibc/kernel floor |
| `--safe` size is tested on the live measure that equals the row's indexed size, and on both when neither does (a reading of R7, put to the foreman for a Clarification) | The index's size mode is not recorded; length alone false-skips small or compressed files selected by disk usage | Recording the size mode is an index-format change (§1), out of scope. "Length or disk usage" (the first draft) let a file that shrank below `--min-size` pass on its block count; the adversarial review caught it |
| Prompt on stderr, where `xdu-rm` uses stdout | R9 keeps stdout for listing and summary; a `> log` redirect would hide the question | Copying `xdu-rm` exactly would contradict R9 |
| `hpccm` smoke runs every `bin/xdu*` by glob instead of naming each binary | The recipe pins 0.6.0, whose tarball has no `xdu-cp`; naming the binaries breaks every image build until the version bump | A `FIRST_WITH_CP` version gate needs a release number not yet decided and an amendment to `xdu-release` steps 3 and 5 |
| A grep gate, not `clippy.toml`, bans replacing renames and copies (`fs::rename`, `fs::copy`, `libc::rename`, `libc::renameat`) and truncating opens (`File::create`, `O_TRUNC`, `.truncate(true)`, `fs::write`) in the new files' non-comment lines, in every phase that touches them | `src/crawl.rs` (high-blast) uses `fs::rename` and `File::create` legitimately; a crate-wide ban would need `#[allow]` edits there | — |

## 4. Rabbit holes (resolved)

- No-replace rename on both OSes and on filesystems without it → raw `SYS_renameat2` /
  `renameatx_np`, `linkat`+`unlinkat` fallback on `EINVAL`/`ENOSYS`/`ENOTSUP`, fail otherwise;
  probed on APFS ([01](research/01-noreplace-rename-atomic-copy.md)).
- Atomic, durable copy and the directory flush → temp in the final directory, `F_FULLFSYNC` /
  `fsync`, flush the directory before any unlink ([01](research/01-noreplace-rename-atomic-copy.md)).
- Cross-filesystem detection → `EXDEV`, not `st_dev`; probed `st_dev` lying across macOS
  volumes ([02](research/02-cross-filesystem-ci.md)).
- Proving the cross-filesystem path in CI → `/dev/shm` and an APFS image, an opt-in tier that
  refuses a vacuous run, and fault-seam unit tests everywhere ([02](research/02-cross-filesystem-ci.md)).
- Bound selection SQL → `read_parquet(?)` works for a local glob; quotes fixed; glob
  metacharacters refused ([03](research/03-engine-selection-executor.md)).
- Shared CLI flags without touching `xdu-find`/`xdu-rm` → flatten, probed identical
  ([03](research/03-engine-selection-executor.md)).
- Packaging lockstep → enumerated with file:line ([04](research/04-packaging-touchpoints.md)).

### Adversarial review of the first draft (2026-11-06)

One reviewer, given the GOAL, the invariants and the draft with no author notes, was tasked to
break it. Fifteen findings; all adopted, one reworked:

| # | Finding | Adopted as |
|---|---------|-----------|
| 1 | Same-filesystem `xdu-mv` checked `S_ISREG` only under `--safe`, so a directory or FIFO at a path would move | §2.5: `fstatat` + `S_ISREG` always; P3 tests a directory and a FIFO without `--safe` |
| 2 | The `linkat` fallback (NFS's main path) unlinked the source without re-checking its inode; a pre-existing hard link read as "moved" | §2.5 step 3: record (dev, ino), re-check before `unlinkat`, roll back our link on a swap; existing pre-check first |
| 3 | A failed source unlink after a full cross-filesystem copy had no defined outcome | §2.5 `move_across` step 5: roll back our destination, count failed; `faccessat` pre-check; a seam test |
| 4 | `xdu-cp` could publish torn bytes stamped with the old mtime | §2.5 step 6: the S1 check runs for both tools |
| 5 | "Length or disk usage" let a shrunken file pass `--safe`, and rewrote R7 inside the plan | Reworked: the measure that matches the indexed size, both otherwise; put to the foreman (§5) |
| 6 | Only `EPERM` counted as an ownership refusal; no group-only retry; the CI owner test would fail on a host whose temp dir has a foreign group | §2.5 step 4; the P2 test sets the source's gid to `getgid()` first |
| 7 | The rename ban was missing from the cross-filesystem phase, matched comments, and missed truncating opens | Every code phase's gate; comment lines filtered; `File::create`, `O_TRUNC`, `.truncate(true)`, `fs::write` added |
| 8 | P5's negative `xfs-wire.sh` checks passed on any failure; the same-filesystem case assumed `TMPDIR` shares `target/`'s filesystem | Gates grep the fixture's own panic text, pin `TMPDIR`, require the script |
| 9 | P5's local gate could not see whether the binary's policy was still `Refuse` | A `relocate` test drives the production policy through a forced `EXDEV` |
| 10 | No `xdu-cp` test for an interrupted copy (R4) | P1 gate: abandon at every step, never a partial final name |
| 11 | Refactoring `to_conditions` in high-blast `lib.rs` was pinned only against itself | No refactor: the engine calls `to_conditions` on a clone with the pattern cleared |
| 12 | Calendar: P1 under-sized, checkpoint keyed on a local gate, packaging scheduled after the cut point | P1 1.5 d; docs and packaging moved before the cross-filesystem phase; checkpoint on CI's `XFS tests`; the cut stated as the expected path |
| 13 | The cut's `st_dev` pre-flight misfires on overlayfs and misses bind mounts | P5-cut keeps P3's per-file refusal; no pre-flight |
| 14 | The source was opened before its type was known (a device swapped in could see an open) | §2.5 walks: `fstatat` first, open only a regular file, compare (dev, ino) |
| 15 | `ORDER BY` would be written twice under `--limit`; P5's prose greps were satisfied by flag text | §2.2 tail rule; the prose gates grep the two required sentences |

## 5. Risks & open questions

- **For the foreman (GOAL Clarification requested):** the `--safe` size reading in §2.2. R7
  says "size against `--min-size`/`--max-size`" without naming the measure, and the index does
  not record which one it stored. The plan's reading: the live measure that equals the indexed
  size, both when neither does. Building proceeds on it unless the gate rules otherwise.

- **Calendar: the cut is the expected path, not a contingency.** Estimated build: P1 1.5 d
  (selection, the two-OS no-replace chain with its fault seam, the executor, the binary, a page
  and about thirty named tests), P2 0.5 d, P3 0.5 d, P4 0.5 d, P5 1.0 d: **4.0 build-days against
  about 3** (GOAL). The core through P4 (3.0 d) fits; P5 does not, with margin. Checkpoint: **if
  P5's local gate is green and CI's `XFS tests` step is green on both OSes by end of
  2026-11-10**, it ships; otherwise the foreman amends the GOAL (R5's cross-filesystem path
  becomes a refusal, seeded as an issue) and P5 is replaced by P5-cut in `TECH.md` (about
  0.25 d), which keeps P3's per-file refusal. Because the checkpoint keys on CI, P5's branch
  needs a CI run on 11-10, which only the foreman can start.
- **CI-only proof.** The `XDU_TEST_XFS_DIR` tier cannot run on the planning or build host
  (every disk-image route was refused by the sandbox). P5's local gate proves the fault-seam
  layer, the tier's existence and that it refuses to pass vacuously; the real `EXDEV` run is
  proven only by CI's `XFS tests` step on both OSes.
- **Not provable anywhere:** behaviour on a real crash or power loss, and that a flush reached
  stable storage. The gates prove the ordering (a recording hook) and whole-somewhere under
  injected failure and abandonment at every step.
- **Destinations without a no-replace rename or hard links** (exFAT, SMB, some FUSE) fail every
  file with a clear message. Correct and blunt; documented.
- **Per-file full flush** (≈2–3 ms on APFS) bounds small-file throughput; accepted for R4/R5.
- **Memory**: the whole selected set is held (needed for R3's whole-set check), as `xdu-rm`
  does; noted, not solved.
- **macOS image ownership**: user-attached volumes ignore ownership, so the owner assertion runs
  on the workspace leg only.
- **Set-id after a refused chown**: dropped by design (§2.5). Untested on BSD semantics beyond
  macOS CI.
- **Possible hardening, not built:** a quarantine rename of the source before the unlink, closing
  the final-name swap race. Recorded in `issues/xdu-mv-quarantine-unlink.md` with a ROADMAP entry
  by P5 (or folded into P5-cut's seed).

## 6. Verification strategy

- **Unit tests** in `select` (SQL text pure-function pins, bound values, glob refusals, the
  `holds_live` ↔ DuckDB boundary grid) and `atomic_fs` (no-replace on both routes, symlink walks,
  fault and abandon at every step, the recording-hook ordering, set-id drop).
- **Integration tests** driving the real binaries: `tests/cp_tests.rs`, `tests/mv_tests.rs`
  (fixtures as `rm_tests.rs`, `--apparent-size` indexes, `--root` canonicalized), plus the opt-in
  `tests/mv_xfs_tests.rs` run by `tests/xfs-wire.sh` in CI.
- **CLI drives** in a throwaway index: `.agents/factory/bin/temp_index.sh sh -c '…'` with
  `--into` under the site and `--root "$(cd tree && pwd -P)"`.
- **Every gate** names its tests and greps `test NAME ... ok` from a log, so a renamed or
  vanished test fails it; runs `cargo test --locked --test rm_tests`, checks
  `git diff --quiet edcc163 -- src/bin/xdu-rm.rs tests/rm_tests.rs`, then clippy `-D warnings` and
  `fmt --check`, and echoes `Pn-GATE-GREEN`.
- **Man pages** are read through `scdoc | mandoc -Tutf8 | col -b` where scdoc exists; it is not
  installed on the planning host, so CI's counted literal check is the render proof.

---

*Backing research: [`research/00-digest.md`](research/00-digest.md).*
