# 00 — Research digest: the decisions the plan takes

Four briefs, read in full. Where they overlap or disagree, this file gives the one answer
[`PLAN.md`](../PLAN.md) builds on.

| # | Brief | Settles |
|---|-------|---------|
| 01 | [`01-noreplace-rename-atomic-copy.md`](01-noreplace-rename-atomic-copy.md) | The no-replace primitive, the copy order, flushing, symlink-safe walks |
| 02 | [`02-cross-filesystem-ci.md`](02-cross-filesystem-ci.md) | How a move crosses filesystems, the unchanged check, the CI proof |
| 03 | [`03-engine-selection-executor.md`](03-engine-selection-executor.md) | Bound selection SQL, the selection API, `--safe`, the executor, tests, CLI |
| 04 | [`04-packaging-touchpoints.md`](04-packaging-touchpoints.md) | Every list a new binary joins (R10, §13) |

## Decisions

1. **No new dependency.** `libc` 0.2.191 already carries `SYS_renameat2`, `RENAME_NOREPLACE`,
   `renameatx_np`, `RENAME_EXCL`, `F_FULLFSYNC`, the `*at` calls, `fchown`, `fchmod` and
   `futimens` (01).
2. **Linux no-replace rename is the raw syscall** `syscall(SYS_renameat2, …, RENAME_NOREPLACE)`,
   not the glibc 2.28 wrapper, so the binary gains no versioned symbol at the floor (01).
   macOS uses `renameatx_np(…, RENAME_EXCL)`.
3. **Fail-closed chain.** Native no-replace rename. On `EINVAL`, `ENOSYS`, `ENOTSUP` or
   `EOPNOTSUPP` (NFS, Lustre, Ceph, 9p, FUSE, non-APFS volumes), fall back to `linkat` then
   `unlinkat`, since `linkat` cannot replace a name. A `linkat` `EEXIST` whose target has the
   source's (dev, ino) is an NFS retransmit that succeeded. Anything else fails the file. No code
   path ever calls plain `rename(2)` or `std::fs::rename`/`std::fs::copy`; a grep gate holds that
   for the new files (01, 02). A `clippy.toml` ban was weighed and rejected because `src/crawl.rs`
   (high-blast) legitimately uses `fs::rename` for chunk finalize.
4. **Copy order** (01, 02 agree): `fstat` the opened source before reading a byte; create
   `.xdu-<pid>-<seq>-<rand>.part` with `O_CREAT|O_EXCL|O_NOFOLLOW`, mode `0600`, in the final
   directory; copy; `fchown` (refusal counted, not a failure; on refusal the set-id bits are
   dropped), then `fchmod`, then `futimens` with nanoseconds; durable flush; no-replace publish;
   durable flush of the directory. 02's temp name `.<name>.xdu-tmp-…` is rejected for 01's, which
   holds no part of the final name and so can never reach `NAME_MAX`.
5. **Durable flush** is `File::sync_all` semantics: `F_FULLFSYNC` on macOS (plain `fsync` leaves
   data in the drive cache), falling back to `fsync` where `F_FULLFSYNC` is refused; `fsync` on
   Linux. Directory fds take the same call; probed to return 0 on APFS (01).
6. **Symlink-safe walks on both sides.** Destination: hold the canonical `--into` as a directory
   fd and descend with `openat(O_DIRECTORY|O_NOFOLLOW)`, creating with `mkdirat` (EEXIST from a
   racing worker is success). Source: descend from `/` the same way and open the file
   `O_NOFOLLOW|O_NONBLOCK`, then `fstat` for `S_ISREG`. Index paths are canonical and were crawled
   without following links, so no legitimate row has a symlink component. A per-worker cache of
   the last parent fd keeps the cost near one `openat` per directory (01).
7. **Cross-filesystem detection is `EXDEV` from the no-replace rename**, never an `st_dev`
   comparison: bind mounts and macOS's sealed system volume share an `st_dev` yet return `EXDEV`
   (02, probed). `st_dev` survives only as the cut's up-front refusal hint.
8. **Existing-destination pre-check.** Before copying, `fstatat(dest_parent, name,
   AT_SYMLINK_NOFOLLOW)`; anything there counts as existing. macOS reports `EEXIST` and Linux
   `EXDEV` for "exists on another device", so without this the count would differ by OS (02).
   The publish stays no-replace regardless.
9. **Source unchanged** (02): S0 = `fstat` at open, S1 = `fstat` after the flush and before the
   publish, S2 = `fstatat(parent, name, NOFOLLOW)` immediately before `unlinkat(parent, name)`.
   Compare size, mtime and ctime with nanoseconds (S0↔S1), plus dev, ino and file type (S0↔S2).
   ctime is included because userspace can restore an mtime but not a ctime. Nothing ever writes
   the source's times.
10. **Bound selection.** A new engine statement, `SELECT path, size, uid, gid, mode, atime, mtime
    FROM read_parquet(?) … ORDER BY path [LIMIT n]`, binding the glob and the regex (03, probed
    against DuckDB 1.4.4). `parquet_source`'s local arm and `QueryFilters::to_conditions` stay
    byte-for-byte; the engine calls `to_conditions` on a clone with the pattern cleared, so the
    predicates exist once and `src/lib.rs` is not refactored.
11. **A bound glob is still a glob** (03, probed: `idx[ab]` silently reads sibling `idxa`;
    `p/../../x` reads another index). The engine refuses, before any SQL, a non-UTF-8 index path
    or one containing `*`, `?`, `[` or `\`, and a partition name that is empty, `.`, `..`, holds
    `/` or any of those characters. A `-u` naming no partition directory is refused clearly instead
    of surfacing DuckDB's "No files found". Escaping as one-character classes works in 1.4.4 but
    is undocumented; it is left to `issues/local-reader-bound-parameters.md`.
12. **Always `ORDER BY path`**, limit or not: R3 names the *first* offending path, dry-run and
    `-v` output become reproducible, and `xdu-tar` wants one ordered stream. Cost: 0.2 s over 5 M
    rows (03).
13. **`--safe` is a pure predicate** `holds_live(&QueryFilters, &Live)` over the thresholds
    `QueryFilters` stored at build time, pinned against the SQL by a boundary grid evaluated in
    DuckDB (03). **Size**: the index's size mode is not recorded (03 risk 1), so the check tests
    the live measure (length or `blocks*512`) that equals the row's indexed size, and both when
    neither does. 03 proposed "either measure"; the plan's adversarial review showed that lets a
    file that shrank below `--min-size` pass on its block count, so it was tightened. The reading
    goes to the foreman as a requested Clarification.
14. **Executor**: a local rayon pool (`-j`), rows processed in ordered chunks, outcomes tallied
    and printed on the main thread, so output order is deterministic. Failures always go to
    stderr; the `-v` listing, dry-run listing, empty-set message and summary go to stdout (the
    summary on stdout keeps `xdu-rm`'s scripts' shape). The prompt goes to stderr. Exit 1 when
    any file failed or already existed (03).
15. **CLI**: a new `SelectArgs` (`#[derive(Args)]`) flattened into new `XduCpArgs` and
    `XduMvArgs`; probed byte-identical help and completions to inline fields. `XduFindArgs` and
    `XduRmArgs` are untouched (03).
16. **CI for the cross-filesystem path** (02): three layers. (1) Unit tests on every OS drive the
    move through a step hook that fails, or abandons without cleanup, at every step, and assert
    the file is whole somewhere; a recording hook asserts no unlink precedes both flushes. (2)
    `#[ignore]`d integration tests read `XDU_TEST_XFS_DIR` and panic unless a probe rename into
    it really returns `EXDEV`. (3) A gate script in the shape of `tests/s3-wire.sh` fails unless
    the passed count equals the listed count. CI provides the directory with `/dev/shm` on
    ubuntu-24.04 (tmpfs, no sudo) and an `hdiutil` APFS image on macos-15 (HFS+ rejected: whole-
    second mtimes). The sandbox here refused every disk-image route, so layer 2 cannot run on
    the planning host; that refusal is recorded evidence, not routed around.
17. **Packaging** (04): the lockstep set is `Cargo.toml`, `gen-completions`, `tests/common`,
    `cli_version_tests`, three `release.yaml` loops, three `packaging/extensions/check.sh` loops
    with the `Dockerfile` in the same commit, `install.sh`, the `hpccm/` recipe and specs, the
    `test.yaml` literal and produced-file lists, the rpm spec, and the `AGENTS.md`/§13 counts.
    The `hpccm` smoke runs every `bin/xdu*` by glob, so the recipe stays correct against the
    pinned 0.6.0 tarball without a version gate.
18. **`doc/xdu-rm.1.scd` stays untouched**, SEE ALSO included: the GOAL keeps every change to
    `xdu-rm` out. Its cross-reference to the new tools moves with
    `issues/xdu-rm-on-shared-engine.md`.
