# Research 02: cross-filesystem moves in `xdu-mv`, and how CI proves them

Scope: R5's cross-filesystem path. This brief covers how the move detects that it is crossing
filesystems, the "source unchanged" check before the unlink, and a CI plan for ubuntu-24.04 and
macos-15 runners. Probes ran on a macOS developer host (Darwin, APFS) inside the agent sandbox,
under `target/tmp/xfsprobe/`.

## 1. Detection: let `EXDEV` decide; use `st_dev` only as a hint

**Recommendation.** Try the no-replace rename first. Take the copy path only on `EXDEV`. Do not
compare `st_dev` to choose the route.

- **Bind mounts (Linux).** `rename(2)` says rename "does not work across different mount points,
  even if the same filesystem is mounted on both". Both sides show the same `st_dev`, but rename
  returns `EXDEV`. A route chosen by comparing `st_dev` would pick the rename, and then the rename
  would fail.
- **macOS shows the same thing.** On this host, the sealed system volume and the Data volume
  report the same `st_dev` but are separate mounts, and rename between them returns `EXDEV`.
- **btrfs subvolumes.** Each subvolume has its own `st_dev`, and rename between subvolumes returns
  `EXDEV`. Here `st_dev` and rename agree.
- **overlayfs.**
  - A non-directory's `st_dev` can come from its layer when the layers span filesystems without
    `xino`, yet renaming it works (overlayfs copies the file up). Comparing `st_dev` would take
    the slow path for nothing.
  - Renaming a directory returns `EXDEV` unless `redirect_dir` is on. That does not matter here,
    because R2 limits the tools to regular files.
- **NFS.** Each client mount has its own `st_dev`. The server can return `EXDEV` across its own
  filesystem boundaries.
  - Footgun for the no-replace research: Linux `nfs_rename` rejects any flags with `EINVAL`, so
    `renameat2(RENAME_NOREPLACE)` fails with `EINVAL` on the same NFS mount.
  - On macOS, `RENAME_EXCL` exists only "on file systems that support it" (`rename(2)`).
  - So the dispatcher should map the errors like this:
    - `EXDEV`: take the copy path.
    - `EEXIST`: skip the file and count it as existing.
    - `EINVAL`, `ENOTSUP` or `ENOSYS`: use the no-replace fallback (for example `link` then
      `unlink`). Never fall back to a plain `rename`.
- **Errno order differs between the OSes (probe below).** With the destination already present on
  another device:
  - macOS `renamex_np(RENAME_EXCL)` returns `EEXIST`.
  - Linux `do_renameat2` checks the mount first and returns `EXDEV`.
  - The copy path must therefore `fstatat(dest, AT_SYMLINK_NOFOLLOW)` before copying. That keeps
    the "existing" count the same on both OSes and avoids a wasted copy. The final publish stays
    no-replace.
- **Where `st_dev` still helps.**
  - **Dry-run.** Dry-run makes zero writes, so it cannot attempt a rename. If it labels routes at
    all, it can only write "(likely copy)". Recommend no route label in dry-run, or label it as a
    hint.
  - **The cut.** If R5's cross-filesystem path is cut to a refusal, an up-front `st_dev`
    comparison can refuse the whole run before any write, in the spirit of R3.
    - The comparison is against the deepest existing ancestor of each mapped destination, not
      `--into`, because a mount point can sit under `DEST`.
    - The per-file `EXDEV` still has to refuse and count as a backstop, because of bind mounts.

## 2. "Source unchanged" before the unlink

**Sequence.** Every step is a point where a test can inject a failure (section 3c).

1. Open the source's parent directory with `O_DIRECTORY|O_NOFOLLOW`. Open the source with
   `openat(pfd, name, O_RDONLY|O_NOFOLLOW|O_CLOEXEC)`.
2. Take `S0 = fstat(fd)`. Require `S_ISREG`.
3. Create the temp file `openat(dpfd, tmp, O_CREAT|O_EXCL|O_WRONLY)` in the destination's parent
   directory. Copy the bytes. Set mode, times and owner on the temp file's descriptor.
4. Flush the temp file: `fsync` on Linux; `fcntl(F_FULLFSYNC)` on macOS. Apple's `fsync(2)` says
   plain `fsync` leaves data in the drive cache. Fall back to `fsync` on `ENOTSUP`.
5. Take `S1 = fstat(fd)` and compare it with S0.
   - If the source changed, unlink the temp file, leave the source alone, and count the file as
     skipped. Nothing ever became visible.
6. Publish the copy: no-replace rename of `tmp` to its final name.
7. Flush the destination's parent directory, with `fsync`, or `F_FULLFSYNC` where it is supported.
8. Take `S2 = fstatat(pfd, name, AT_SYMLINK_NOFOLLOW)`, then immediately run
   `unlinkat(pfd, name, 0)`.
   - If S2 does not match, remove the published destination (after checking that its inode is
     the temp file's inode). Count the file as skipped. The source stays.

**Fields to compare.**

| Comparison | Fields |
|---|---|
| S0 to S1 | `st_size`, `st_mtime` with nanoseconds, `st_ctime` with nanoseconds |
| S0 to S2 | `st_dev`, `st_ino`, file type `S_IFREG`, `st_size`, `st_mtime` with nanoseconds, `st_ctime` with nanoseconds |

- `st_dev` and `st_ino` in S2 are what catch a rename-swap of the path.
- `nlink` is left out. Any change to the link count bumps `ctime`.
- Atime is left out, because the copy itself reads the file.

**Why include ctime.**

- Userspace can set mtime to any value: `utimes`, `touch -d`, tar, rsync. A writer that restores
  the mtime defeats the GOAL's size-plus-mtime check. Userspace cannot set ctime, and every write,
  chmod, chown, link, rename or utimes bumps it.
- Footgun: the move must never `utimes` the source, for example to restore its atime. That bumps
  ctime, and the move would then skip every file it touched.

**Races that remain.** These go in the man page as limits.

- **Write after the last check.** A write between S2 and `unlinkat` is lost. The window is two
  syscalls long. Neither OS has mandatory locks; `flock` binds only writers that cooperate.
- **An open writer descriptor.** After the unlink, its writes land on the orphaned inode and are
  lost. A writer that does not write during the copy is invisible to every check.
- **Timestamp granularity.** Linux stamps times from a coarse clock (one tick, a few ms), so a
  same-size overwrite within one tick changes neither mtime nor ctime. On HFS+, times have
  1-second resolution.
- **`mmap` writers.** A `MAP_SHARED` write updates mtime only on the first write fault after
  writeback. The check is best-effort for these.
- **Rename-swap of the final name between S2 and `unlinkat`.** Unlinking relative to the held
  parent descriptor closes swaps of ancestor directories and symlinks above the parent, but not a
  swap of the last name inside the same directory. POSIX has no unlink-if-inode-matches.
- **Possible hardening, not recommended this cycle.**
  - **Quarantine rename.** Rename the source no-replace to a private
    `.xdu-mv-<pid>-<n>` in the same directory, `fstatat` it and check its inode against S0, then
    unlink it. This closes the swap. A crash can leave a quarantine name behind, but the
    destination is already durable, so nothing is lost.
  - **Linux `F_SETLEASE F_RDLCK` on the source descriptor.** It fails with `EAGAIN` if anyone has
    the file open for writing, so it detects open writers. It needs the caller to own the file
    (or `CAP_LEASE`) and does not work on NFS.

**Crash safety.** "Whole somewhere" holds at every step boundary because:

- the unlink is issued only after steps 4 and 7 have returned;
- the final name only ever receives a complete, flushed temp file.

A crash leaves at worst a stray temp file. Name temp files `.<name>.xdu-tmp-<pid>-<n>` so a later
cleanup can find them.

## 3. CI strategy, with no root needed for the default leg

**a. ubuntu-24.04**

- **`/dev/shm`** is the tmpfs that systemd always mounts (an "API filesystem"), with mode `1777`.
  Writing there needs no sudo, and it is a separate mount from `$GITHUB_WORKSPACE` and from
  `/tmp`. GitHub keeps `/tmp` on the root disk, even on the 26.04 image (computingforgeeks,
  Oct 2026).
- **`/mnt`** was a separate temp disk on the 20.04 and 22.04 images. I could not verify it for
  24.04, and it is root-owned.
- **Use `/dev/shm`.** Make the step assert that the device numbers differ, so a change to the
  image fails the step loudly.
- **Should a step use `sudo mount`?** The runners have passwordless sudo, but not for the main leg,
  because developers' Linux boxes must be able to reproduce it.
- **Optional, cheap, and the best proof of section 1:** a second leg using
  `sudo mount --bind "$RUNNER_TEMP/xfs-a" "$RUNNER_TEMP/xfs-b"`. It has the same `st_dev` but
  rename returns `EXDEV`, so a route chosen by `st_dev` would go red on it.

**b. macos-15**

- **On this host the sandbox refused every mount route** (evidence in section 4). The refusals
  come from the sandbox blocking DiskArbitration and DiskImages, not from the OS needing root.
- **On hosted runners**, the runner user is an unsandboxed admin, and `hdiutil create` and
  `hdiutil attach` work without sudo.
  - Known flake: "create failed - Resource busy" (actions/runner-images#7522). The fix is a
    retry loop with `-ov`.
- **Use an APFS image, not an HFS+ RAM disk.** HFS+ stores times in whole seconds, so R2's mtime
  tests would fail. APFS keeps nanoseconds.
- **Ownership is ignored on user-attached volumes.** `st_uid` then reads back as the caller, so
  the owner test proves less there. Either assert ownership on the workspace leg, or run
  `sudo diskutil enableOwnership "$mnt"`.
- **Deprecation.** On this Darwin host, `hdiutil create -size … -volname` and
  `hdiutil attach -nomount` print warnings saying they are deprecated in favour of
  `diskutil image …`. Pin the step to `hdiutil` for macos-15 and expect to change it on a later
  image.

**Step sketch.** Add it before `Test`. The gate is a new `XFS tests` step in the style of
`S3 tests`.

```yaml
      - name: Second filesystem for the cross-filesystem move tests (Linux)
        if: runner.os == 'Linux'
        run: |
          set -eu
          d=/dev/shm/xdu-xfs; mkdir -p "$d"
          test "$(stat -c %d "$d")" != "$(stat -c %d "$GITHUB_WORKSPACE")"
          echo "XDU_TEST_XFS_DIR=$d" >> "$GITHUB_ENV"
      - name: Second filesystem for the cross-filesystem move tests (macOS)
        if: runner.os == 'macOS'
        run: |
          set -eu
          img="$RUNNER_TEMP/xdu-xfs.dmg"; d="$RUNNER_TEMP/xdu-xfs"; mkdir -p "$d"
          for i in 1 2 3; do hdiutil create -ov -size 256m -fs APFS -volname xdu-xfs "$img" && break; sleep 5; done
          hdiutil attach -nobrowse -mountpoint "$d" "$img"
          test "$(stat -f %d "$d")" != "$(stat -f %d "$GITHUB_WORKSPACE")"
          echo "XDU_TEST_XFS_DIR=$d" >> "$GITHUB_ENV"
      # ... Test ...
      - name: XFS tests
        run: sh tests/xfs-wire.sh "" N   # N = listed count of tests/mv_xfs_tests.rs; or generalize s3-wire.sh
```

**c. Seams, in three layers**

| Layer | Runs | Proves | Does not prove |
|---|---|---|---|
| **1. Unit tests, every OS, plain `cargo test`** | A crate-private `move_across(src, dst, hook)` with a step hook `FnMut(Step) -> HookAction`, where the action is `Continue`, `Fail(io::Error)` or `Abandon`. Production passes `Continue`. Also an injectable `rename_noreplace` that returns `EXDEV`. | <ul><li>The dispatcher takes the copy path on `EXDEV`.</li><li>For each step `k`, `Fail` leaves the source byte-identical, no final name and no temp file, and counts the file as failed.</li><li>`Abandon` is a simulated crash with no cleanup. After it, the source is whole or the final name is whole, and the final name is never partial.</li><li>A source mutated at step `k` (an append, a same-size overwrite plus `utimes` back to the old mtime) is kept, with no destination, and counted as skipped. The `utimes` case is what makes ctime necessary.</li></ul> | That a real `EXDEV` happens, and real flush semantics |
| **2. Ignored integration tests, opt-in** (`tests/mv_xfs_tests.rs`) | Each test is `#[ignore = "needs XDU_TEST_XFS_DIR on a second filesystem"]`. The fixture panics if the variable is unset, or if a probe rename into it does not return `EXDEV`. That rules out a vacuous pass and checks the real condition rather than `st_dev`. Each test uses its own subdirectory. | <ul><li>The real binary on a real second mount: the source is gone, and the destination's bytes, mode, mtime and atime match.</li><li>The destination's `st_dev` differs from the source's.</li><li>The same-filesystem leg keeps the inode (rename was used).</li><li>An existing destination is skipped and counted.</li></ul> | Crash behaviour; that the flush reached the platter |
| **3. CI gate** | The gate script fails unless the passed count equals the listed count and reaches the minimum, as `s3-wire.sh` does. | Layer 2 ran on both OSes | (as above) |

**Cannot be proven in CI:**

- a real crash or power loss between steps;
- that `fsync` or `F_FULLFSYNC` reached stable storage (virtual disks may ack flushes they do not
  honour);
- races with concurrent writers.

What the gate proves instead:

- the ordering: no unlink before the flush calls have returned, checked with a recording hook
  that asserts the step sequence;
- the whole-somewhere property under an injected failure or abandonment at every step.

## 4. Probe evidence (this macOS host, in the sandbox)

```
stat -f 'dev=%d' $TMPDIR /tmp /private/var/folders <worktree> target/tmp  -> all dev=16777229 (disk3s1, Data)
python os.rename(target/tmp/xfsprobe/src.txt, X):
  /dev/x                       EXDEV  (devfs, dev 1715286606)
  /System/Volumes/VM/x         EXDEV  (dev 16777234)
  /usr/x                       EXDEV  (parent st_dev=16777229 == src st_dev; statvfs f_fsid 16777235 vs 16777229)
renamex_np(src, X, RENAME_EXCL):
  /dev/null (exists, other dev) EEXIST   <- macOS checks existence before device
  /System/Volumes/VM/x          EXDEV
  same-dir existing             EEXIST ; same-dir new  OK
hdiutil attach -nomount ram://65536          -> "attach failed - Device not configured" (+ deprecation warning)
diskutil image attach --noMount ram://65536  -> "unable to use the DiskManagement framework"
hdiutil create -size 16m -fs APFS|HFS+ …     -> "create failed - Device not configured"
```

- No second writable filesystem can be made on a sandboxed developer host, so layer 2 shows as
  ignored there.
- Linux was not probed. The section 3a facts come from documentation and are guarded by the
  step's `stat` assertion.

## Risks

- **Linux runner layout.** If the image ever moves `/dev/shm` onto the workspace's device, the
  step goes red, which is the intended behaviour.
- **macOS image steps.** `hdiutil` can flake with "Resource busy", is deprecated going forward,
  and user-attached volumes ignore ownership.
- **Coarse timestamps.** A same-size rewrite within one clock tick evades both mtime and ctime.
  Document it.
- **Name swap at unlink.** A rename-swap of the source's name between S2 and `unlinkat` is
  narrowed by the held parent descriptor but not closed. The quarantine rename would close it;
  this cycle leaves it as a documented limit.
- **NFS.** `EINVAL` from `RENAME_NOREPLACE` must route to the no-replace fallback and never to a
  plain `rename`.
- **Box size.** Layer 1 plus the gate is the bulk of the work. If the cut applies, layer 1's
  fault-injection tests move with the seeded path, and only the `EXDEV` refusal test stays.

Sources: Linux `rename(2)` (EXDEV, mount points); macOS `rename(2)` and `fsync(2)` on this host;
systemd API file systems; [actions/runner-images#7522](https://github.com/actions/runner-images/issues/7522);
[GitHub changelog, Ubuntu 24.04 GA](https://github.blog/changelog/2024-09-25-actions-new-images-and-ubuntu-latest-changes/);
[free-disk-space README (older-image `/mnt` disk)](https://github.com/jlumbroso/free-disk-space);
[computingforgeeks: ubuntu-latest migration](https://computingforgeeks.com/github-actions-ubuntu-latest-migration/).
