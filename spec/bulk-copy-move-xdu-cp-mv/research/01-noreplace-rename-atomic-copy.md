# Research 01 — no-replace rename and the atomic, durable copy primitive

Serves R2, R4, R5 (and R7's re-stat). Linux facts are cited from man pages and kernel source
(this host is macOS and cannot probe Linux); macOS facts are probed. Probes live in
`target/tmp/noreplace-probe/` (throwaway, uncommitted).

## Findings

**`libc` 0.2.191 already has everything; add no dependency.**
- Linux gnu and musl: `renameat2` (`src/unix/linux_like/linux/{gnu,musl}/mod.rs`),
  `RENAME_NOREPLACE: c_uint = 1` (`linux/mod.rs:1183`), `SYS_renameat2` (x86_64 316, aarch64 276),
  `syscall`, `copy_file_range`, `fdatasync`.
- Apple: `renamex_np`, `renameatx_np`, `RENAME_EXCL = 0x4` (`bsd/apple/mod.rs:3557,5010`),
  `F_FULLFSYNC = 51`, `O_NOFOLLOW_ANY`, `fclonefileat`, `fcopyfile`.
- Both: `openat`, `mkdirat`, `linkat`, `unlinkat`, `fstatat`, `fsync`, `futimens`, `fchmod`,
  `fchown`, `O_DIRECTORY`, `O_NOFOLLOW`, `O_CLOEXEC`, `AT_SYMLINK_NOFOLLOW`.
- `rustix` and `filetime` are not direct dependencies. `rustix 0.38` reaches the runtime only
  through `crossterm`. `rustix 1.1` (via `tempfile`, `xattr`) and `filetime` (via `tar`) are
  dev/build-only. Taking `rustix` with its `fs` feature directly would buy safe wrappers for
  about ten calls. `libc` is already the runtime dep, and the crate already has 7 `unsafe` sites
  in `lib.rs`, so a small `unsafe` module is the cheaper option.

**glibc floor.** The `renameat2` wrapper is `GLIBC_2.28`, exactly the floor. The assert
(`release.yaml:164-176`) checks max ≤ 2.28, so it passes. glibc's wrapper is a thin syscall with
no emulation, so `libc::syscall(libc::SYS_renameat2, …)` has identical semantics, carries no
versioned symbol, and keeps the floor flexible. **Recommend the raw syscall**; the wrapper is
acceptable. Separately, the assert loop names only `xdu xdu-find xdu-view xdu-rm`, so the new
binaries must be added (R10 lockstep).

**Filesystem support for a native no-replace rename (Linux).** `rename(2)` lists ext4 (3.15),
btrfs, tmpfs and cifs (3.17), xfs (4.0), and ext2/vfat/jfs/… (4.9). overlayfs accepts
`RENAME_NOREPLACE`. Unsupported filesystems return **EINVAL**: `nfs_rename` begins
`if (flags) return -EINVAL;` (fs/nfs/dir.c, master). The 2016 `.rename2` conversion did the same
for 9p, afs, ceph, coda, ecryptfs, lustre, ocfs2 and orangefs. FUSE with a server that lacks
rename2 also reports EINVAL. GPFS is out-of-tree and unknown, so treat it as possibly EINVAL.
Old kernels and seccomp return ENOSYS. The man page BUGS section warns that on NFS a failed op
may still have happened (RPC retransmit).

**macOS.** `rename(2)` documents RENAME_EXCL "on file systems that support it
(VOL_CAP_INT_RENAME_EXCL)", with **ENOTSUP** otherwise and EEXIST when the target exists.
`fsync(2)` states that it does not flush the drive cache, and that power loss can lose or reorder
writes; F_FULLFSYNC does. **Rust's `File::sync_all` and `sync_data` already issue F_FULLFSYNC on
Apple** (timed in probe B). They do not fall back on filesystems that reject it, so wrap them.

## Probe evidence (macOS 27, APFS, `cc` and `rustc 1.97.1`)

A. `probe.c` (`cc -o probe probe.c && ./probe .`)
```
renameatx_np(a->b, RENAME_EXCL) [b exists] -> -1 errno=17 (File exists)
  a: "SRC-A"   b: "DEST-B"                       # destination untouched
renamex_np(a->c, RENAME_EXCL) [c free] -> 0      # a absent, c: "SRC-A"
linkat(c->b) [b exists] -> -1 errno=17 (File exists)   # b still "DEST-B"
linkat(c->d) [d free] -> 0 ; c nlink=2 ; unlinkat(c) -> 0 ; d: "SRC-A"
fsync(dirfd O_RDONLY|O_DIRECTORY) -> 0
fcntl(dirfd, F_FULLFSYNC) -> 0
fsync(file) -> 0 ; fcntl(file, F_FULLFSYNC) -> 0
openat(ln, O_DIRECTORY|O_NOFOLLOW) [ln->.] -> -1 errno=20 (Not a directory)
openat(ln, O_RDONLY|O_NOFOLLOW) -> -1 errno=62 (ELOOP)
mkdirat(ln) [exists as symlink] -> -1 errno=17
openat(dang, O_CREAT|O_EXCL) [dangling symlink] -> -1 errno=17 ; target not created
timing: fsync 0.030 ms/op, F_FULLFSYNC 1.798 ms/op (n=50)
```
B. `syncall.rs`: `raw fsync 0.029–0.044 ms/op, File::sync_all 2.3–2.8, File::sync_data 2.6–3.0`.
That is F_FULLFSYNC's cost, so std uses it.

C. `nfany.c`: `open(lnk/f, O_NOFOLLOW)` succeeds, because the parent symlink is followed.
`open(lnk/f, O_NOFOLLOW_ANY)` fails with ELOOP. `open(fifo, O_RDONLY|O_NONBLOCK)` returns with no
hang.

D. The probe on a second filesystem was **refused by the sandbox**: `hdiutil create -fs MS-DOS` →
"create failed - Device not configured". It was not retried; only APFS is mounted. The ENOTSUP
path on HFS+, FAT, exFAT and SMB stays unprobed.

## Recommended design

The module name `engine` is taken (DuckDB connections, `src/engine.rs`). Put the primitive in a
new **`src/atomic_fs.rs`** (or `fsops.rs`) that does not depend on the select-act engine, so
`xdu-tar` reuses it as-is. It needs `#[cfg(unix)]` and a `libc`, `OwnedFd` and `CStr` API:

```rust
pub struct DirHandle(OwnedFd);                       // O_RDONLY|O_DIRECTORY|O_CLOEXEC
pub fn open_into(into: &Path) -> io::Result<DirHandle>;           // operator's --into, resolved once
pub fn walk_nofollow(base: &DirHandle, rel_dirs: &[&OsStr], create: bool) -> io::Result<DirHandle>;
pub fn open_source(parent: &DirHandle, name: &OsStr) -> io::Result<(File, Stat)>; // NOFOLLOW|NONBLOCK, fstat, S_ISREG
pub enum Placed { Done, Existing, OwnerNotPreserved }   // R9 counters
pub fn rename_noreplace(sp: &DirHandle, sn: &OsStr, dp: &DirHandle, dn: &OsStr) -> Result<(), NoReplaceErr>;
pub fn copy_atomic(src: &File, st: &Stat, dest: &DirHandle, name: &OsStr, opts: CopyOpts) -> io::Result<Placed>;
pub fn sync_durable(fd: BorrowedFd) -> io::Result<()>;  // Apple: F_FULLFSYNC, on ENOTSUP/EINVAL/ENOTTY → fsync; else fsync
// NoReplaceErr: Existing | CrossDevice(EXDEV) | Unsupported | Io(io::Error)
```
Mirror `clippy.toml`'s DuckDB rule: add `std::fs::rename` (and `std::fs::copy`, which
overwrites) to `disallowed-methods`, and put `#[allow]` only in this module, so "never a plain
`rename(2)`" is enforced by the lint.

**Fail-closed no-replace chain** (`rename_noreplace`):
1. Linux: `syscall(SYS_renameat2, sp, sn, dp, dn, RENAME_NOREPLACE)`. Apple:
   `renameatx_np(…, RENAME_EXCL)`.
2. Result 0 → done. EEXIST → `Existing`. EXDEV → `CrossDevice` (`xdu-mv` takes the copy path).
3. EINVAL, ENOSYS or ENOTSUP/EOPNOTSUPP → `linkat(sp, sn, dp, dn, 0)`. linkat cannot replace a
   name, so falling back on *any* of these errnos is safe; the list only keeps EACCES and ENOENT
   from being masked. On link success: `sync_durable(dp)`, then `unlinkat(sp, sn, 0)`.
   - linkat EEXIST → `fstatat(dp, dn, NOFOLLOW)`. If the (dev, ino) matches the source, this was
     an NFS retransmit of a link that succeeded, so continue. Otherwise → `Existing`.
   - linkat EPERM, ENOTSUP or EMLINK (FAT, exFAT, some FUSE, SMB) → `Unsupported`: the file fails
     with "destination supports neither no-replace rename nor hard links".
   - unlinkat ENOENT after a verified link → done (NFS retransmit).
4. Any other errno → the file fails. There is no `rename(2)` and no O_EXCL-placeholder trick: a
   placeholder holds an empty file at the final name (breaks R4) and the swap races.
5. Optional: cache "no native flag" per `st_dev` (an atomic in a small map) to skip a call that
   cannot succeed.

**`copy_atomic` order** (in the destination directory, so the publish never crosses a device):
1. Before `open_source` reads any bytes, `fstat` the source and keep atime, mtime, size, uid, gid
   and mode. A read may bump atime, so the pre-read stamp is the one carried.
2. Temp: `openat(dest, ".xdu-<pid>-<seq>-<rand64hex>.part", O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC, 0600)`.
   The name has a fixed prefix and suffix, so `find DEST -name '.xdu-*.part'` finds leftovers. It
   holds no part of the final name, so NAME_MAX is never hit. pid, sequence and random make a
   clash with a selected name improbable, and O_EXCL plus the no-replace publish make it harmless
   (it counts as Existing, never a clobber).
3. Copy bytes with `std::io::copy` (copy_file_range on Linux; reflink and sparse handling are
   whatever std does). Unlink the temp on any error.
4. `fchown(uid, gid)`. EPERM → `OwnerNotPreserved`, which is not a failure. Then `fchmod(mode & 07777)`:
   chown clears S_ISUID and S_ISGID, root included since Linux 2.2.13. Then
   `futimens([atime, mtime])` with nanoseconds, last, because the earlier calls touch ctime only.
5. `sync_durable(temp)`.
6. `xdu-mv` only: re-`fstat` the source fd. If size or mtime changed → unlink the temp, Skipped
   (changed).
7. Publish: `rename_noreplace(dest, temp, dest, final)`. `Existing` → unlink the temp, Existing.
8. `sync_durable(dest)`. This is mandatory for `xdu-mv`; on failure, unlink the final name we
   created, keep the source, and the file fails. For `xdu-cp` the plan may make it best-effort,
   but the file fsync in step 5 stays mandatory: without it, XFS or ext4 without `auto_da_alloc`
   can expose a zero-length final name after power loss, which breaks R4.
9. `xdu-mv` only: `fstatat(src_parent, name, NOFOLLOW)` must still match the fd's (dev, ino) and
   size, or leave both copies and fail. Then `unlinkat(src_parent, name, 0)`, then
   `sync_durable(src_parent)` (best-effort; a lost unlink only restores "both").

A same-filesystem `xdu-mv` is `rename_noreplace` on the source and destination parents directly:
the inode, owner, mode and times carry over, so it never yields OwnerNotPreserved.

## Crash-window table

| Crash after… | Source | Final name | Debris | R4/R5 |
|---|---|---|---|---|
| 2 temp created | whole | absent | empty `.part` | ok |
| 3–4 copying / metadata | whole | absent | partial `.part` | ok |
| 5 fsync temp | whole | absent | complete `.part` | ok |
| 7 native rename publish | whole | complete (atomic) | none | ok |
| 7 linkat fallback, before temp unlink | whole | complete | `.part` (same inode) | ok |
| 7, power loss before 8 | whole | maybe lost | maybe `.part` | ok: source not yet unlinked |
| 8 dest dir durable | whole | complete | — | both |
| 9 unlink, before src dir fsync | gone or restored | complete | — | dest or both |
| same-fs native rename | one of the two names (atomic) | | — | ok |
| same-fs link fallback, between link and unlink | whole | complete | — | both |

The step 8 durable directory sync before step 9 is what prevents "neither" across two devices.

## Symlink safety: recommend walking both sides with nofollow

- **Destination (strong yes).** Hold `--into` once. Then, per component,
  `openat(parent, c, O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`. On ENOENT, `mkdirat(parent, c, 0777)`
  (umask applies), treating EEXIST from a parallel worker as success, then reopen. ELOOP or
  ENOTDIR → the file fails ("destination component is not a directory"). Without this, a symlink
  planted in a shared cold-storage tree redirects a root run to create files carrying
  user-controlled content in any directory (e.g. a `*.d` config directory). No-replace stops
  overwrites, not creation.
- **Source (yes, it is nearly free).** Index paths are canonicalized and crawled without following
  links (`src/bin/xdu.rs:612-615`), so a legitimate path has no symlink component and a nofollow
  walk from `/` (or the `--root` fd) has no false positives. It closes a parent-swap race: a root
  `xdu-mv` could otherwise rename or unlink a same-named file under a symlink target. It also
  gives `--safe` an `fstatat(parent, name, AT_SYMLINK_NOFOLLOW)` (true lstat semantics, fixing
  the follow-the-link gap the GOAL cites in `xdu-rm`). Open with O_NOFOLLOW|O_NONBLOCK, then
  `fstat` the fd for S_ISREG; checking the fd, not the path, means a FIFO neither hangs (probe C)
  nor slips past.
- **Cost:** one `openat` per path component per file. Selection is `ORDER BY path`, so a
  per-worker cache of the last parent `DirHandle` (keyed by its relative dir) makes it about one
  `openat` per directory. Later: `O_NOFOLLOW_ANY` (macOS 11+, probed) or `openat2`
  `RESOLVE_NO_SYMLINKS` (Linux 5.6+, not on RHEL8's 4.18) could replace the walk.

## Testing notes

- On ubuntu-24.04, `/dev/shm` (tmpfs) against the ext4 workspace gives a real EXDEV, so the
  cross-filesystem path can be tested on Linux. macOS CI has one APFS volume.
- Every CI filesystem supports the native flag, so the linkat fallback needs an injectable seam
  (e.g. `CopyOpts { force_link_fallback }`), as does the chown-refusal counter.

## Open risks

1. Destinations with neither a no-replace rename nor hard links (exFAT, SMB, some FUSE, macOS
   FAT) fail every file. That is correct but blunt; the man page should say so. The macOS ENOTSUP
   path is unprobed (D).
2. GPFS and newer Lustre behaviour is unverified. The chain is safe whatever errno they return,
   but they may cost an extra syscall per file.
3. Step 9 TOCTOU: there is no portable "unlink only if the inode matches". The window between the
   identity check and `unlinkat` remains, which matters only against a hostile writer in the
   source directory.
4. fchmod with S_ISGID after a refused fchown: Linux silently drops the bit, BSD may differ. Test
   it and decide whether a dropped special bit counts as "ownership not preserved".
5. Per-file F_FULLFSYNC (≈2–3 ms on APFS) and NFS COMMIT bound small-file throughput. That is
   accepted as the price of R5; batching the directory fsync per directory is a later
   optimisation.
6. EPERM from a seccomp filter on `renameat2` is not in the fallback set, so the file fails
   closed. Add EPERM only if CI containers show it.
