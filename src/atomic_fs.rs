//! Filesystem primitives that place a file without ever replacing or truncating one.
//!
//! Every operation works on directory file descriptors walked one component at a time with
//! `O_NOFOLLOW`, so a symlink planted in a shared destination tree cannot redirect a write and a
//! source parent swapped for a symlink cannot redirect a read. A copy is written under a temp
//! name in its final directory, flushed, and published by a rename that refuses to replace, so a
//! final name only ever holds a complete file. Nothing here calls `rename(2)`, which silently
//! replaces its destination, or opens a file with truncation.

use std::collections::hash_map::RandomState;
use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// An open directory, the anchor for every `*at` call.
#[derive(Debug)]
pub struct DirHandle(OwnedFd);

impl DirHandle {
    /// Open a directory the operator named, following symlinks in it as the shell would.
    ///
    /// Only the operator's own `--into` and `/` are opened this way; everything beneath them is
    /// reached through `open_dir_nofollow`.
    pub fn open(path: &Path) -> io::Result<DirHandle> {
        let c = c_name(path.as_os_str())?;
        // SAFETY: `c` is a valid NUL-terminated string for the duration of the call.
        let fd = unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        Ok(DirHandle(owned(fd)?))
    }

    fn raw(&self) -> RawFd {
        self.0.as_raw_fd()
    }

    fn try_clone(&self) -> io::Result<DirHandle> {
        Ok(DirHandle(self.0.try_clone()?))
    }
}

impl AsFd for DirHandle {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} contains a NUL byte", name.to_string_lossy()),
        )
    })
}

fn owned(fd: RawFd) -> io::Result<OwnedFd> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: a non-negative return from open/openat is a fresh descriptor we own.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

fn check(ret: libc::c_int) -> io::Result<()> {
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn errno_is(e: &io::Error, codes: &[libc::c_int]) -> bool {
    e.raw_os_error().is_some_and(|code| codes.contains(&code))
}

/// True when a walk stopped because a component was a symlink or not a directory.
pub fn is_not_a_directory(e: &io::Error) -> bool {
    errno_is(e, &[libc::ENOTDIR, libc::ELOOP])
}

/// Open `name` in `parent` as a directory, refusing a symlink.
pub fn open_dir_nofollow(parent: &DirHandle, name: &OsStr) -> io::Result<DirHandle> {
    let c = c_name(name)?;
    // SAFETY: `parent` is an open descriptor and `c` a valid C string for the call.
    let fd = unsafe {
        libc::openat(
            parent.raw(),
            c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    Ok(DirHandle(owned(fd)?))
}

/// The `Normal` components of a path; a root, `.` or `..` contributes nothing.
///
/// Callers hand in paths already checked to be normalized, so nothing is dropped in practice.
pub fn components(path: &Path) -> Vec<&OsStr> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect()
}

/// Descend from `base` through `names` without following any symlink or creating anything.
pub fn walk_nofollow(base: &DirHandle, names: &[&OsStr]) -> io::Result<DirHandle> {
    let mut current = base.try_clone()?;
    for name in names {
        current = open_dir_nofollow(&current, name)?;
    }
    Ok(current)
}

/// Descend from `base` through `names`, creating each missing directory, never following a
/// symlink.
///
/// A component that exists as anything but a directory fails the walk: a symlink there could
/// point a privileged run at any directory on the host, and refusing to create the replica's
/// directory is the only safe answer. Created directories take mode `0777` less the umask; a
/// racing worker's `EEXIST` is success.
pub fn walk_create_nofollow(base: &DirHandle, names: &[&OsStr]) -> io::Result<DirHandle> {
    let mut current = base.try_clone()?;
    for name in names {
        let next = match open_dir_nofollow(&current, name) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let c = c_name(name)?;
                let mode: libc::mode_t = 0o777;
                // SAFETY: `current` is open and `c` a valid C string for the call.
                let made = check(unsafe { libc::mkdirat(current.raw(), c.as_ptr(), mode) });
                match made {
                    Ok(()) => {}
                    Err(e) if errno_is(&e, &[libc::EEXIST]) => {}
                    Err(e) => return Err(e),
                }
                open_dir_nofollow(&current, name).map_err(|e| not_a_directory(e, name))?
            }
            Err(e) => return Err(not_a_directory(e, name)),
        };
        current = next;
    }
    Ok(current)
}

fn not_a_directory(e: io::Error, name: &OsStr) -> io::Error {
    if is_not_a_directory(&e) {
        io::Error::new(
            e.kind(),
            format!(
                "destination component {} is not a directory",
                name.to_string_lossy()
            ),
        )
    } else {
        e
    }
}

/// The fields of `struct stat` this module compares, widened to one type on every platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub size: i64,
    pub blocks: i64,
    pub atime: i64,
    pub atime_nsec: i64,
    pub mtime: i64,
    pub mtime_nsec: i64,
    pub ctime: i64,
    pub ctime_nsec: i64,
}

impl Stat {
    // The libc field types differ between Linux and macOS (`st_mode` is u16 on one, u32 on the
    // other), so a cast that widens on one platform is a no-op on the other.
    #[allow(clippy::unnecessary_cast)]
    fn from_libc(st: &libc::stat) -> Stat {
        Stat {
            dev: st.st_dev as u64,
            ino: st.st_ino as u64,
            mode: st.st_mode as u32,
            nlink: st.st_nlink as u64,
            uid: st.st_uid as u32,
            gid: st.st_gid as u32,
            size: st.st_size as i64,
            blocks: st.st_blocks as i64,
            atime: st.st_atime as i64,
            atime_nsec: st.st_atime_nsec as i64,
            mtime: st.st_mtime as i64,
            mtime_nsec: st.st_mtime_nsec as i64,
            ctime: st.st_ctime as i64,
            ctime_nsec: st.st_ctime_nsec as i64,
        }
    }

    /// True for a regular file; false for a symlink, directory, FIFO, socket or device.
    #[allow(clippy::unnecessary_cast)]
    pub fn is_regular(&self) -> bool {
        self.mode & (libc::S_IFMT as u32) == (libc::S_IFREG as u32)
    }

    /// The same filesystem object: one device, one inode.
    pub fn same_object(&self, other: &Stat) -> bool {
        self.dev == other.dev && self.ino == other.ino
    }
}

/// `lstat` semantics relative to a directory: the object at `name` itself, never a link target.
pub fn fstatat_nofollow(dir: &DirHandle, name: &OsStr) -> io::Result<Stat> {
    let c = c_name(name)?;
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `st` is written by a successful call before it is read.
    check(unsafe {
        libc::fstatat(
            dir.raw(),
            c.as_ptr(),
            st.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    // SAFETY: fstatat returned 0, so it filled `st`.
    let st = unsafe { st.assume_init() };
    Ok(Stat::from_libc(&st))
}

/// `fstat` of an open descriptor.
pub fn fstat(fd: BorrowedFd<'_>) -> io::Result<Stat> {
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `st` is written by a successful call before it is read.
    check(unsafe { libc::fstat(fd.as_raw_fd(), st.as_mut_ptr()) })?;
    // SAFETY: fstat returned 0, so it filled `st`.
    let st = unsafe { st.assume_init() };
    Ok(Stat::from_libc(&st))
}

fn unlink_at(dir: &DirHandle, name: &OsStr) -> io::Result<()> {
    let c = c_name(name)?;
    // SAFETY: `dir` is open and `c` a valid C string for the call.
    check(unsafe { libc::unlinkat(dir.raw(), c.as_ptr(), 0) })
}

/// Remove `name` only while it is still `expected`'s inode, so a name someone else replaced
/// in the meantime is left alone.
fn unlink_if_same(dir: &DirHandle, name: &OsStr, expected: &Stat) -> io::Result<()> {
    match fstatat_nofollow(dir, name) {
        Ok(there) if there.same_object(expected) => unlink_at(dir, name),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// A regular source file opened for reading, with the `fstat` taken at open.
#[derive(Debug)]
pub struct SourceFile {
    file: File,
    stat: Stat,
}

impl SourceFile {
    pub fn stat(&self) -> &Stat {
        &self.stat
    }
}

/// What lies at a source path.
#[derive(Debug)]
pub enum Source {
    Regular(SourceFile),
    /// Nothing there, or a parent directory is gone.
    Missing,
    /// A symlink, directory, FIFO, socket or device, or a parent that is not a directory.
    NotRegular,
    /// The name was replaced between the type check and the open.
    Changed,
}

/// Open `name` in `parent` for reading, only if it is a regular file.
///
/// The type is judged by `fstatat(AT_SYMLINK_NOFOLLOW)` before anything is opened, so a device
/// driver never sees an open and a FIFO never blocks the worker. The open itself is
/// `O_NOFOLLOW|O_NONBLOCK`, and its `fstat` must name the inode the type check saw.
pub fn open_source(parent: &DirHandle, name: &OsStr) -> io::Result<Source> {
    let seen = match fstatat_nofollow(parent, name) {
        Ok(seen) => seen,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Source::Missing),
        Err(e) => return Err(e),
    };
    if !seen.is_regular() {
        return Ok(Source::NotRegular);
    }
    let c = c_name(name)?;
    // SAFETY: `parent` is open and `c` a valid C string for the call.
    let fd = unsafe {
        libc::openat(
            parent.raw(),
            c.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    let fd = match owned(fd) {
        Ok(fd) => fd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Source::Missing),
        Err(e) if errno_is(&e, &[libc::ELOOP]) => return Ok(Source::Changed),
        Err(e) => return Err(e),
    };
    let file = File::from(fd);
    let stat = fstat(file.as_fd())?;
    if !stat.is_regular() || !stat.same_object(&seen) {
        return Ok(Source::Changed);
    }
    Ok(Source::Regular(SourceFile { file, stat }))
}

/// The last directory handle a worker walked to, keyed by the path it was walked from.
///
/// Selections arrive in path order, so consecutive files share a parent and a worker pays about
/// one `openat` per directory rather than one per component per file.
#[derive(Debug, Default)]
pub struct ParentCache {
    key: Option<PathBuf>,
    dir: Option<DirHandle>,
}

impl ParentCache {
    pub fn get(
        &mut self,
        key: &Path,
        walk: impl FnOnce() -> io::Result<DirHandle>,
    ) -> io::Result<&DirHandle> {
        if self.key.as_deref() != Some(key) || self.dir.is_none() {
            self.key = None;
            self.dir = Some(walk()?);
            self.key = Some(key.to_path_buf());
        }
        Ok(self.dir.as_ref().expect("walked above"))
    }
}

/// Why a no-replace rename did not place the file.
#[derive(Debug)]
pub enum NoReplace {
    /// Something already holds the destination name; it was not touched.
    Existing,
    /// The source and destination are on different filesystems.
    CrossDevice,
    /// The destination supports neither a no-replace rename nor hard links.
    Unsupported,
    Io(io::Error),
}

impl NoReplace {
    /// The error a caller reports for a refusal that is not `Existing`.
    pub fn into_io(self) -> io::Error {
        match self {
            NoReplace::Existing => io::Error::from_raw_os_error(libc::EEXIST),
            NoReplace::CrossDevice => io::Error::from_raw_os_error(libc::EXDEV),
            NoReplace::Unsupported => io::Error::new(
                io::ErrorKind::Unsupported,
                "destination supports neither a no-replace rename nor hard links",
            ),
            NoReplace::Io(e) => e,
        }
    }
}

/// The points in a placement where a test can fail it or abandon it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    TempCreated,
    BytesWritten,
    /// The copy carries the source's owner and group, or as much of them as was permitted.
    Owned,
    /// The copy carries the source's permission bits.
    Moded,
    /// The copy carries the source's atime and mtime.
    Timed,
    TempSynced,
    /// A move found a regular file at the source and nothing at the destination, and has not
    /// yet renamed it.
    Cleared,
    /// The link route made the new name and has not yet removed the old one.
    Linked,
    Published,
    DirSynced,
}

/// Every step `copy_atomic` passes on the native route, in order.
#[cfg(test)]
pub(crate) const COPY_STEPS: [Step; 8] = [
    Step::TempCreated,
    Step::BytesWritten,
    Step::Owned,
    Step::Moded,
    Step::Timed,
    Step::TempSynced,
    Step::Published,
    Step::DirSynced,
];

// Production passes `NoHook`, which only ever continues; the other actions are the tests'.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum HookAction {
    Continue,
    Fail(io::Error),
    /// Stop at once with no cleanup, as a crash would.
    Abandon,
}

pub(crate) trait StepHook {
    fn at(&mut self, step: Step) -> HookAction;
}

pub(crate) struct NoHook;

impl StepHook for NoHook {
    fn at(&mut self, _: Step) -> HookAction {
        HookAction::Continue
    }
}

/// Overrides that reach paths a test host cannot otherwise take.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Faults {
    /// Skip the native no-replace rename and publish through `linkat`, as on NFS.
    pub force_link_route: bool,
    /// Refuse the owner-and-group `fchown` with `EPERM`, as for a caller who is not root.
    pub refuse_owner: bool,
    /// Refuse the group-only `fchown` retry with `EPERM` too.
    pub refuse_group: bool,
    /// Answer every no-replace rename with `EXDEV`, as a destination on another filesystem
    /// does.
    pub force_cross_device: bool,
}

/// A placement stopped either by an error or, under a test hook, by a simulated crash.
#[derive(Debug)]
pub(crate) enum Stop<E> {
    Err(E),
    Abandoned,
}

impl From<io::Error> for Stop<io::Error> {
    fn from(e: io::Error) -> Self {
        Stop::Err(e)
    }
}

impl From<NoReplace> for Stop<NoReplace> {
    fn from(e: NoReplace) -> Self {
        Stop::Err(e)
    }
}

impl From<io::Error> for Stop<NoReplace> {
    fn from(e: io::Error) -> Self {
        Stop::Err(NoReplace::Io(e))
    }
}

fn step<E: From<io::Error>>(hook: &mut dyn StepHook, at: Step) -> Result<(), Stop<E>> {
    match hook.at(at) {
        HookAction::Continue => Ok(()),
        HookAction::Fail(e) => Err(Stop::Err(E::from(e))),
        HookAction::Abandon => Err(Stop::Abandoned),
    }
}

impl From<io::Error> for NoReplace {
    fn from(e: io::Error) -> Self {
        NoReplace::Io(e)
    }
}

/// Rename `sn` in `sp` to `dn` in `dp`, never replacing anything at `dn`.
///
/// The native call is `renameat2(RENAME_NOREPLACE)` on Linux, issued as a raw syscall so the
/// binary carries no versioned glibc symbol, and `renameatx_np(RENAME_EXCL)` on macOS. A
/// filesystem without the flag (NFS, most FUSE servers, older kernels) answers `EINVAL`,
/// `ENOSYS` or `ENOTSUP`, and the rename falls back to `linkat` then `unlinkat`, which can
/// never replace a name either. Any other error fails closed: there is no plain `rename(2)`
/// anywhere on this path.
pub fn rename_noreplace(
    sp: &DirHandle,
    sn: &OsStr,
    dp: &DirHandle,
    dn: &OsStr,
) -> Result<(), NoReplace> {
    match rename_noreplace_with(sp, sn, dp, dn, Faults::default(), &mut NoHook) {
        Ok(()) => Ok(()),
        Err(Stop::Err(e)) => Err(e),
        Err(Stop::Abandoned) => Err(NoReplace::Io(io::Error::other("rename abandoned"))),
    }
}

pub(crate) fn rename_noreplace_with(
    sp: &DirHandle,
    sn: &OsStr,
    dp: &DirHandle,
    dn: &OsStr,
    faults: Faults,
    hook: &mut dyn StepHook,
) -> Result<(), Stop<NoReplace>> {
    let csn = c_name(sn).map_err(NoReplace::Io)?;
    let cdn = c_name(dn).map_err(NoReplace::Io)?;
    if faults.force_cross_device {
        return Err(Stop::Err(NoReplace::CrossDevice));
    }
    if !faults.force_link_route {
        match native_rename_noreplace(sp, &csn, dp, &cdn) {
            Ok(()) => return Ok(()),
            Err(e) if errno_is(&e, &[libc::EEXIST]) => return Err(Stop::Err(NoReplace::Existing)),
            Err(e) if errno_is(&e, &[libc::EXDEV]) => {
                return Err(Stop::Err(NoReplace::CrossDevice));
            }
            Err(e)
                if errno_is(
                    &e,
                    &[libc::EINVAL, libc::ENOSYS, libc::ENOTSUP, libc::EOPNOTSUPP],
                ) => {}
            Err(e) => return Err(Stop::Err(NoReplace::Io(e))),
        }
    }
    link_route(sp, sn, &csn, dp, dn, &cdn, hook)
}

#[cfg(target_os = "linux")]
fn native_rename_noreplace(
    sp: &DirHandle,
    sn: &CString,
    dp: &DirHandle,
    dn: &CString,
) -> io::Result<()> {
    // SAFETY: both descriptors are open and both names valid C strings for the call.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            sp.raw(),
            sn.as_ptr(),
            dp.raw(),
            dn.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn native_rename_noreplace(
    sp: &DirHandle,
    sn: &CString,
    dp: &DirHandle,
    dn: &CString,
) -> io::Result<()> {
    // SAFETY: both descriptors are open and both names valid C strings for the call.
    check(unsafe {
        libc::renameatx_np(
            sp.raw(),
            sn.as_ptr(),
            dp.raw(),
            dn.as_ptr(),
            libc::RENAME_EXCL,
        )
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn native_rename_noreplace(
    _sp: &DirHandle,
    _sn: &CString,
    _dp: &DirHandle,
    _dn: &CString,
) -> io::Result<()> {
    Err(io::Error::from_raw_os_error(libc::ENOSYS))
}

/// The `linkat` + `unlinkat` publish for filesystems without a native no-replace rename.
///
/// On NFS this is the main path, so it carries its own identity checks. The source's inode is
/// recorded first. After the link, the destination directory is flushed and the source name
/// re-checked: if it no longer names the recorded inode, someone swapped it in the window, and
/// unlinking it would remove a file nobody selected, so the new link is withdrawn and the
/// placement fails. An `EEXIST` from `linkat` that names the recorded inode is a retransmitted
/// NFS request that already succeeded.
fn link_route(
    sp: &DirHandle,
    sn: &OsStr,
    csn: &CString,
    dp: &DirHandle,
    dn: &OsStr,
    cdn: &CString,
    hook: &mut dyn StepHook,
) -> Result<(), Stop<NoReplace>> {
    let recorded = fstatat_nofollow(sp, sn)?;
    // SAFETY: both descriptors are open and both names valid C strings for the call.
    let linked = check(unsafe { libc::linkat(sp.raw(), csn.as_ptr(), dp.raw(), cdn.as_ptr(), 0) });
    if let Err(e) = linked {
        if errno_is(&e, &[libc::EEXIST]) {
            let there = fstatat_nofollow(dp, dn)?;
            if !there.same_object(&recorded) {
                return Err(Stop::Err(NoReplace::Existing));
            }
        } else if errno_is(&e, &[libc::EXDEV]) {
            return Err(Stop::Err(NoReplace::CrossDevice));
        } else if errno_is(
            &e,
            &[libc::EPERM, libc::ENOTSUP, libc::EOPNOTSUPP, libc::EMLINK],
        ) {
            return Err(Stop::Err(NoReplace::Unsupported));
        } else {
            return Err(Stop::Err(NoReplace::Io(e)));
        }
    }

    let verified = (|| -> Result<(), Stop<NoReplace>> {
        step::<NoReplace>(hook, Step::Linked)?;
        sync_durable(dp.as_fd())?;
        let now = fstatat_nofollow(sp, sn);
        let still = matches!(&now, Ok(s) if s.same_object(&recorded));
        if !still {
            return Err(Stop::Err(NoReplace::Io(io::Error::other(
                "source name changed during the publish; it was left in place",
            ))));
        }
        Ok(())
    })();
    if let Err(stop) = verified {
        if let Stop::Err(_) = stop {
            withdraw_link(dp, dn, &recorded);
        }
        return Err(stop);
    }

    match unlink_at(sp, sn) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            withdraw_link(dp, dn, &recorded);
            Err(Stop::Err(NoReplace::Io(e)))
        }
    }
}

/// Remove the destination link the link route made, but only while it still names the
/// recorded inode and that inode has another name: withdrawing the last name of a file would
/// lose it, and a placement that fails must leave the data somewhere.
fn withdraw_link(dp: &DirHandle, dn: &OsStr, recorded: &Stat) {
    if let Ok(there) = fstatat_nofollow(dp, dn)
        && there.same_object(recorded)
        && there.nlink > 1
    {
        let _ = unlink_at(dp, dn);
    }
}

/// Flush a file or directory to stable storage.
///
/// On macOS `fsync` does not flush the drive's cache and power loss can lose or reorder
/// writes; `F_FULLFSYNC` does, and falls back to `fsync` on a filesystem that rejects it.
pub fn sync_durable(fd: BorrowedFd<'_>) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: `fd` is open for the duration of the call.
        let ret = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_FULLFSYNC) };
        if ret != -1 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if !errno_is(&e, &[libc::ENOTSUP, libc::EINVAL, libc::ENOTTY]) {
            return Err(e);
        }
    }
    // SAFETY: `fd` is open for the duration of the call.
    check(unsafe { libc::fsync(fd.as_raw_fd()) })
}

/// Where a copy landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placed {
    /// Published. `owner_kept` is false when the owner or the group could not be carried.
    Done { owner_kept: bool },
    /// Something already held the destination name; it was not touched and no byte was read.
    Existing,
    /// The source was written while it was read, so the bytes may be torn; nothing was
    /// published.
    SourceChanged,
}

/// Which of the source's ids the copy carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Carried {
    owner: bool,
    group: bool,
}

// `uid_t` and `gid_t` are `u32` on every supported platform, so these casts only name the
// C types.
#[allow(clippy::unnecessary_cast)]
fn fchown_with(fd: BorrowedFd<'_>, uid: u32, gid: u32, refuse: bool) -> io::Result<()> {
    if refuse {
        return Err(io::Error::from_raw_os_error(libc::EPERM));
    }
    // SAFETY: `fd` is open for the duration of the call.
    check(unsafe { libc::fchown(fd.as_raw_fd(), uid as libc::uid_t, gid as libc::gid_t) })
}

/// Give the copy the source's owner and group, as far as the caller is permitted.
///
/// Any error is a refusal, not a failure: `EPERM` for a caller who is not root, `EINVAL` for
/// an id an NFSv4 server cannot map, `EACCES` or `ENOTSUP` elsewhere. A refused owner retries
/// with the group alone, which a caller who belongs to the group may set.
fn carry_ids(fd: BorrowedFd<'_>, src: &Stat, faults: Faults) -> Carried {
    if fchown_with(fd, src.uid, src.gid, faults.refuse_owner).is_ok() {
        return Carried {
            owner: true,
            group: true,
        };
    }
    // An id of -1 leaves the owner as it is.
    let group = fchown_with(fd, u32::MAX, src.gid, faults.refuse_group).is_ok();
    Carried {
        owner: false,
        group,
    }
}

/// The permission bits the copy takes: the source's, less the set-id bits when either id was
/// not carried, as `cp -p` does, so a refused `chown` never yields a set-id file owned by the
/// caller.
// `mode_t` is `u16` on macOS and `u32` on Linux, so a cast that converts on one is a no-op on
// the other.
#[allow(clippy::unnecessary_cast)]
fn carried_mode(src: &Stat, carried: Carried) -> libc::mode_t {
    let mut mode = src.mode & 0o7777;
    if !(carried.owner && carried.group) {
        mode &= !(libc::S_ISUID as u32 | libc::S_ISGID as u32);
    }
    mode as libc::mode_t
}

/// Stamp the copy with the source's atime and mtime, nanoseconds included.
// `time_t` and the nanosecond field are `i64` on the 64-bit targets and narrower elsewhere.
#[allow(clippy::unnecessary_cast)]
fn carry_times(fd: BorrowedFd<'_>, src: &Stat) -> io::Result<()> {
    let times = [
        libc::timespec {
            tv_sec: src.atime as libc::time_t,
            tv_nsec: src.atime_nsec as _,
        },
        libc::timespec {
            tv_sec: src.mtime as libc::time_t,
            tv_nsec: src.mtime_nsec as _,
        },
    ];
    // SAFETY: `fd` is open and `times` a two-element array for the duration of the call.
    check(unsafe { libc::futimens(fd.as_raw_fd(), times.as_ptr()) })
}

/// True when the source's size, mtime or ctime moved since `before` was taken: something
/// wrote it, or rewrote it and restored its mtime, which only the ctime shows.
fn source_changed(before: &Stat, now: &Stat) -> bool {
    (
        before.size,
        before.mtime,
        before.mtime_nsec,
        before.ctime,
        before.ctime_nsec,
    ) != (
        now.size,
        now.mtime,
        now.mtime_nsec,
        now.ctime,
        now.ctime_nsec,
    )
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// `.xdu-<pid>-<seq>-<random>.part`: never part of a final name, so a leftover from a crash
/// cannot be mistaken for a copy, and `find DEST -name '.xdu-*.part'` finds every one.
fn temp_name() -> OsString {
    let seq = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(seq);
    hasher.write_u32(std::process::id());
    OsString::from(format!(
        ".xdu-{}-{}-{:016x}.part",
        std::process::id(),
        seq,
        hasher.finish()
    ))
}

/// Copy `src` to `name` in `dest`, atomically and without replacing anything.
///
/// The bytes go to a temp file created `O_EXCL` in the destination directory. The temp then
/// takes the source's owner and group where permitted, its permission bits, and its atime and
/// mtime, all from the `fstat` taken when the source was opened, in that order: `chown` clears
/// the set-id bits, so the mode follows it, and the times come last so nothing moves them. The
/// temp is flushed, the source is re-checked, and the temp is published by
/// `rename_noreplace`; then the directory is flushed. Without the file flush, XFS or ext4 can
/// expose a zero-length final name after power loss. A source whose size, mtime or ctime moved
/// during the copy was written while it was read, so its temp is removed and nothing is
/// published. On any error the temp is removed, and a final name this call published is
/// withdrawn, so a failed copy leaves nothing at its name; a crash leaves at most a
/// `.xdu-*.part` file.
pub fn copy_atomic(src: &SourceFile, dest: &DirHandle, name: &OsStr) -> io::Result<Placed> {
    match copy_atomic_with(src, dest, name, Faults::default(), &mut NoHook) {
        Ok(placed) => Ok(placed),
        Err(Stop::Err(e)) => Err(e),
        Err(Stop::Abandoned) => Err(io::Error::other("copy abandoned")),
    }
}

pub(crate) fn copy_atomic_with(
    src: &SourceFile,
    dest: &DirHandle,
    name: &OsStr,
    faults: Faults,
    hook: &mut dyn StepHook,
) -> Result<Placed, Stop<io::Error>> {
    match fstatat_nofollow(dest, name) {
        Ok(_) => return Ok(Placed::Existing),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(Stop::Err(e)),
    }

    let temp = temp_name();
    let c = c_name(&temp)?;
    let perm: libc::c_uint = 0o600;
    // SAFETY: `dest` is open and `c` a valid C string for the call.
    let fd = unsafe {
        libc::openat(
            dest.raw(),
            c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            perm,
        )
    };
    let mut out = File::from(owned(fd)?);

    let mut published: Option<Stat> = None;
    let result = (|| -> Result<Placed, Stop<io::Error>> {
        step::<io::Error>(hook, Step::TempCreated)?;
        io::copy(&mut &src.file, &mut out)?;
        step::<io::Error>(hook, Step::BytesWritten)?;
        let carried = carry_ids(out.as_fd(), &src.stat, faults);
        step::<io::Error>(hook, Step::Owned)?;
        // SAFETY: `out` is open for the duration of the call.
        check(unsafe { libc::fchmod(out.as_raw_fd(), carried_mode(&src.stat, carried)) })?;
        step::<io::Error>(hook, Step::Moded)?;
        carry_times(out.as_fd(), &src.stat)?;
        step::<io::Error>(hook, Step::Timed)?;
        sync_durable(out.as_fd())?;
        step::<io::Error>(hook, Step::TempSynced)?;
        if source_changed(&src.stat, &fstat(src.file.as_fd())?) {
            return Ok(Placed::SourceChanged);
        }
        let written = fstat(out.as_fd())?;
        match rename_noreplace_with(dest, &temp, dest, name, faults, hook) {
            Ok(()) => {}
            Err(Stop::Err(NoReplace::Existing)) => return Ok(Placed::Existing),
            Err(Stop::Err(other)) => return Err(Stop::Err(other.into_io())),
            Err(Stop::Abandoned) => return Err(Stop::Abandoned),
        }
        published = Some(written);
        step::<io::Error>(hook, Step::Published)?;
        sync_durable(dest.as_fd())?;
        step::<io::Error>(hook, Step::DirSynced)?;
        Ok(Placed::Done {
            owner_kept: carried.owner && carried.group,
        })
    })();

    match result {
        Err(Stop::Abandoned) => Err(Stop::Abandoned),
        Ok(done @ Placed::Done { .. }) => Ok(done),
        other => {
            if let (Err(_), Some(written)) = (&other, &published) {
                let _ = unlink_if_same(dest, name, written);
            }
            match unlink_at(dest, &temp) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) if other.is_ok() => return Err(Stop::Err(e)),
                Err(_) => {}
            }
            other
        }
    }
}

/// What a same-filesystem move did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    /// Renamed. The inode travels with the name, and its owner, mode and times with it.
    Done,
    /// Nothing at the source name.
    Missing,
    /// A symlink, directory, FIFO, socket or device holds the source name; it was not moved.
    NotRegular,
    /// The caller's check turned the file down, for the reason given; it was not moved.
    Rejected(&'static str),
    /// Something already holds the destination name; neither name was touched.
    Existing,
    /// The destination is on another filesystem; the source was not touched.
    CrossDevice,
}

/// Move the regular file `sn` in `sp` to `dn` in the directory `dest` yields, by a rename that
/// never replaces anything.
///
/// The object at the source name is judged by `fstatat(AT_SYMLINK_NOFOLLOW)` first, on every
/// call: a rename moves whatever holds the name, so without this a directory would carry its
/// whole subtree and a FIFO or symlink would move as itself. Only a regular file goes on to
/// `admit`, the caller's own check of its metadata (`--safe`), and only then is the
/// destination directory reached (and created), so nothing is created for a file that is not
/// moved. A name already taken at the destination is reported before the rename is tried, and
/// the rename itself refuses one taken in between. An object swapped in at the source name
/// after the check is renamed as itself and never followed: a rename does not follow a final
/// symlink.
pub fn move_same_fs<'d>(
    sp: &DirHandle,
    sn: &OsStr,
    dest: impl FnOnce() -> io::Result<&'d DirHandle>,
    dn: &OsStr,
    admit: impl FnOnce(&Stat) -> Result<(), &'static str>,
) -> io::Result<Moved> {
    move_same_fs_with(sp, sn, dest, dn, admit, Faults::default(), &mut NoHook)
}

pub(crate) fn move_same_fs_with<'d>(
    sp: &DirHandle,
    sn: &OsStr,
    dest: impl FnOnce() -> io::Result<&'d DirHandle>,
    dn: &OsStr,
    admit: impl FnOnce(&Stat) -> Result<(), &'static str>,
    faults: Faults,
    hook: &mut dyn StepHook,
) -> io::Result<Moved> {
    let seen = match fstatat_nofollow(sp, sn) {
        Ok(seen) => seen,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Moved::Missing),
        Err(e) => return Err(e),
    };
    if !seen.is_regular() {
        return Ok(Moved::NotRegular);
    }
    if let Err(reason) = admit(&seen) {
        return Ok(Moved::Rejected(reason));
    }
    let dp = dest()?;
    match fstatat_nofollow(dp, dn) {
        Ok(_) => return Ok(Moved::Existing),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let renamed = match step::<NoReplace>(hook, Step::Cleared) {
        Ok(()) => rename_noreplace_with(sp, sn, dp, dn, faults, hook),
        Err(stop) => Err(stop),
    };
    match renamed {
        Ok(()) => Ok(Moved::Done),
        Err(Stop::Err(NoReplace::Existing)) => Ok(Moved::Existing),
        Err(Stop::Err(NoReplace::CrossDevice)) => Ok(Moved::CrossDevice),
        Err(Stop::Err(NoReplace::Io(e))) if e.kind() == io::ErrorKind::NotFound => {
            // The source may have gone in the meantime; a vanished destination directory is a
            // failure, not a missing file.
            match fstatat_nofollow(sp, sn) {
                Err(gone) if gone.kind() == io::ErrorKind::NotFound => Ok(Moved::Missing),
                _ => Err(e),
            }
        }
        Err(Stop::Err(other)) => Err(other.into_io()),
        Err(Stop::Abandoned) => Err(io::Error::other("move abandoned")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use tempfile::TempDir;

    fn put(path: &Path, bytes: &[u8]) {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        f.write_all(bytes).unwrap();
    }

    fn read(path: &Path) -> Vec<u8> {
        let mut bytes = Vec::new();
        fs::File::open(path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }

    fn dir(path: &Path) -> DirHandle {
        DirHandle::open(path).unwrap()
    }

    #[allow(clippy::unnecessary_cast)]
    fn set_times(path: &Path, atime: (i64, i64), mtime: (i64, i64)) {
        let c = CString::new(path.as_os_str().as_bytes()).unwrap();
        let times = [
            libc::timespec {
                tv_sec: atime.0 as libc::time_t,
                tv_nsec: atime.1 as _,
            },
            libc::timespec {
                tv_sec: mtime.0 as libc::time_t,
                tv_nsec: mtime.1 as _,
            },
        ];
        // SAFETY: a valid C string and a two-element array for the call.
        let ret = unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), 0) };
        assert_eq!(ret, 0);
    }

    fn parts_in(path: &Path) -> Vec<String> {
        fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".xdu-") && n.ends_with(".part"))
            .collect()
    }

    fn mkfifo(path: &Path) {
        let c = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid C string for the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }

    fn open_regular(parent: &Path, name: &str) -> SourceFile {
        match open_source(&dir(parent), OsStr::new(name)).unwrap() {
            Source::Regular(f) => f,
            other => panic!("{name}: {other:?}"),
        }
    }

    /// A source large enough that a partial copy is distinguishable from a whole one.
    fn big_source(path: &Path) -> Vec<u8> {
        let bytes: Vec<u8> = (0..(1 << 20)).map(|i| (i % 251) as u8).collect();
        put(path, &bytes);
        bytes
    }

    struct At {
        step: Step,
        action: fn() -> HookAction,
    }

    impl StepHook for At {
        fn at(&mut self, step: Step) -> HookAction {
            if step == self.step {
                (self.action)()
            } else {
                HookAction::Continue
            }
        }
    }

    fn fail() -> HookAction {
        HookAction::Fail(io::Error::other("injected"))
    }

    fn abandon() -> HookAction {
        HookAction::Abandon
    }

    fn routes() -> [(Faults, &'static [Step]); 2] {
        const LINK: [Step; 9] = [
            Step::TempCreated,
            Step::BytesWritten,
            Step::Owned,
            Step::Moded,
            Step::Timed,
            Step::TempSynced,
            Step::Linked,
            Step::Published,
            Step::DirSynced,
        ];
        [
            (Faults::default(), &COPY_STEPS),
            (
                Faults {
                    force_link_route: true,
                    ..Faults::default()
                },
                &LINK,
            ),
        ]
    }

    #[test]
    fn publish_never_replaces_an_existing_name() {
        let t = TempDir::new().unwrap();
        put(&t.path().join("a"), b"NEW");
        put(&t.path().join("b"), b"OLD");
        let d = dir(t.path());
        let got = rename_noreplace(&d, OsStr::new("a"), &d, OsStr::new("b"));
        assert!(matches!(got, Err(NoReplace::Existing)), "{got:?}");
        assert_eq!(read(&t.path().join("a")), b"NEW");
        assert_eq!(read(&t.path().join("b")), b"OLD");

        rename_noreplace(&d, OsStr::new("a"), &d, OsStr::new("c")).unwrap();
        assert!(!t.path().join("a").exists());
        assert_eq!(read(&t.path().join("c")), b"NEW");

        let src = t.path().join("src");
        fs::create_dir(&src).unwrap();
        put(&src.join("f"), b"SOURCE");
        let out = t.path().join("out");
        fs::create_dir(&out).unwrap();
        put(&out.join("f"), b"KEEP");
        let placed = copy_atomic(&open_regular(&src, "f"), &dir(&out), OsStr::new("f")).unwrap();
        assert_eq!(placed, Placed::Existing);
        assert_eq!(read(&out.join("f")), b"KEEP");
        assert!(parts_in(&out).is_empty());
    }

    #[test]
    fn the_link_fallback_never_replaces_an_existing_name() {
        let t = TempDir::new().unwrap();
        put(&t.path().join("a"), b"NEW");
        put(&t.path().join("b"), b"OLD");
        let d = dir(t.path());
        let link = Faults {
            force_link_route: true,
            ..Faults::default()
        };
        let got =
            rename_noreplace_with(&d, OsStr::new("a"), &d, OsStr::new("b"), link, &mut NoHook);
        assert!(
            matches!(got, Err(Stop::Err(NoReplace::Existing))),
            "{got:?}"
        );
        assert_eq!(read(&t.path().join("a")), b"NEW");
        assert_eq!(read(&t.path().join("b")), b"OLD");

        rename_noreplace_with(&d, OsStr::new("a"), &d, OsStr::new("c"), link, &mut NoHook).unwrap();
        assert!(!t.path().join("a").exists());
        assert_eq!(read(&t.path().join("c")), b"NEW");
        assert_eq!(fs::metadata(t.path().join("c")).unwrap().nlink(), 1);

        let src = t.path().join("src");
        fs::create_dir(&src).unwrap();
        put(&src.join("f"), b"SOURCE");
        let out = t.path().join("out");
        fs::create_dir(&out).unwrap();
        let placed = copy_atomic_with(
            &open_regular(&src, "f"),
            &dir(&out),
            OsStr::new("f"),
            link,
            &mut NoHook,
        );
        assert!(matches!(placed, Ok(Placed::Done { .. })), "{placed:?}");
        assert_eq!(read(&out.join("f")), b"SOURCE");
        assert!(parts_in(&out).is_empty());
    }

    /// Between the link and the unlink, the source name is swapped for another file. The
    /// re-check sees a different inode, withdraws the new link, and unlinks nothing.
    #[test]
    fn the_link_fallback_rechecks_the_source_before_unlinking() {
        let t = TempDir::new().unwrap();
        let root = t.path().to_path_buf();
        put(&root.join("a"), b"ORIGINAL");
        put(&root.join("intruder"), b"INTRUDER");

        struct Swap(PathBuf);
        impl StepHook for Swap {
            fn at(&mut self, step: Step) -> HookAction {
                if step == Step::Linked {
                    fs::hard_link(self.0.join("a"), self.0.join("aside")).unwrap();
                    fs::remove_file(self.0.join("a")).unwrap();
                    fs::hard_link(self.0.join("intruder"), self.0.join("a")).unwrap();
                }
                HookAction::Continue
            }
        }

        let d = dir(&root);
        let link = Faults {
            force_link_route: true,
            ..Faults::default()
        };
        let got = rename_noreplace_with(
            &d,
            OsStr::new("a"),
            &d,
            OsStr::new("b"),
            link,
            &mut Swap(root.clone()),
        );
        assert!(matches!(got, Err(Stop::Err(NoReplace::Io(_)))), "{got:?}");
        assert!(!root.join("b").exists(), "the new link was withdrawn");
        assert_eq!(
            read(&root.join("a")),
            b"INTRUDER",
            "the swapped-in file stays"
        );
        assert_eq!(read(&root.join("aside")), b"ORIGINAL");
    }

    #[test]
    fn a_failed_copy_leaves_nothing_at_the_final_name() {
        for (faults, steps) in routes() {
            for &at in steps {
                let t = TempDir::new().unwrap();
                let src = t.path().join("src");
                let out = t.path().join("out");
                fs::create_dir(&src).unwrap();
                fs::create_dir(&out).unwrap();
                let bytes = big_source(&src.join("f"));
                let got = copy_atomic_with(
                    &open_regular(&src, "f"),
                    &dir(&out),
                    OsStr::new("f"),
                    faults,
                    &mut At {
                        step: at,
                        action: fail,
                    },
                );
                assert!(
                    matches!(got, Err(Stop::Err(_))),
                    "{at:?} {faults:?}: {got:?}"
                );
                assert!(
                    !out.join("f").exists(),
                    "{at:?} {faults:?}: final name present"
                );
                assert!(parts_in(&out).is_empty(), "{at:?} {faults:?}: temp left");
                assert_eq!(read(&src.join("f")), bytes, "{at:?}: source changed");
            }
        }
    }

    #[test]
    fn copy_atomic_abandoned_at_every_step_never_leaves_a_partial_final_name() {
        for (faults, steps) in routes() {
            for &at in steps {
                let t = TempDir::new().unwrap();
                let src = t.path().join("src");
                let out = t.path().join("out");
                fs::create_dir(&src).unwrap();
                fs::create_dir(&out).unwrap();
                let bytes = big_source(&src.join("f"));
                let got = copy_atomic_with(
                    &open_regular(&src, "f"),
                    &dir(&out),
                    OsStr::new("f"),
                    faults,
                    &mut At {
                        step: at,
                        action: abandon,
                    },
                );
                assert!(matches!(got, Err(Stop::Abandoned)), "{at:?}: {got:?}");
                let final_name = out.join("f");
                if final_name.exists() {
                    assert_eq!(read(&final_name), bytes, "{at:?} {faults:?}: partial final");
                }
                let published = matches!(at, Step::Linked | Step::Published | Step::DirSynced);
                assert_eq!(final_name.exists(), published, "{at:?} {faults:?}");
                assert_eq!(read(&src.join("f")), bytes);
            }
        }
    }

    fn chgrp(path: &Path, gid: u32) {
        std::os::unix::fs::lchown(path, None, Some(gid)).unwrap();
    }

    fn chmod(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn egid() -> u32 {
        // SAFETY: getegid cannot fail.
        unsafe { libc::getegid() }
    }

    /// A group the caller belongs to other than `not`, if it has one.
    fn another_group(not: u32) -> Option<u32> {
        // SAFETY: a zero size only counts.
        let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
        let mut groups = vec![0 as libc::gid_t; n.max(0) as usize];
        // SAFETY: `groups` holds `n` entries.
        let got = unsafe { libc::getgroups(n, groups.as_mut_ptr()) };
        groups.truncate(got.max(0) as usize);
        std::iter::once(egid()).chain(groups).find(|g| *g != not)
    }

    /// A source directory and an output directory under one temp dir, and a source file in
    /// the caller's own group with `mode`.
    fn setid_fixture(mode: u32) -> (TempDir, PathBuf, PathBuf) {
        let t = TempDir::new().unwrap();
        let src = t.path().join("src");
        let out = t.path().join("out");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&out).unwrap();
        put(&src.join("f"), b"SETID");
        chgrp(&src.join("f"), egid());
        chmod(&src.join("f"), mode);
        (t, src, out)
    }

    /// The set-id bits the host let the fixture's source keep. A sandbox can strip them on
    /// `chmod`; where it does, the set-id assertions hold trivially and the CI legs, which keep
    /// them, are where they bite.
    fn setid_kept(src: &Path) -> u32 {
        let kept = fs::metadata(src.join("f")).unwrap().mode() & 0o6000;
        if kept == 0 {
            eprintln!("this host strips set-id bits on chmod; the set-id check runs on CI");
        }
        kept
    }

    fn refusing(owner: bool, group: bool) -> Faults {
        Faults {
            refuse_owner: owner,
            refuse_group: group,
            ..Faults::default()
        }
    }

    /// Between reading the source and publishing the copy, the source is appended to, or
    /// rewritten at the same size with its mtime put back. Either way the copy may hold torn
    /// bytes, so nothing is published.
    #[test]
    fn a_source_written_during_the_copy_is_skipped_and_nothing_published() {
        struct Scribble(PathBuf, bool);
        impl StepHook for Scribble {
            fn at(&mut self, step: Step) -> HookAction {
                if step == Step::BytesWritten {
                    let path = self.0.join("f");
                    let before = fs::metadata(&path).unwrap();
                    let mut f = fs::OpenOptions::new().write(true).open(&path).unwrap();
                    if self.1 {
                        f.write_all(b"REWRITE").unwrap();
                        drop(f);
                        let mtime = (before.mtime(), before.mtime_nsec());
                        let atime = (before.atime(), before.atime_nsec());
                        set_times(&path, atime, mtime);
                        let after = fs::metadata(&path).unwrap();
                        assert_eq!(
                            (after.len(), after.mtime(), after.mtime_nsec()),
                            (before.len(), mtime.0, mtime.1)
                        );
                    } else {
                        use std::io::Seek;
                        f.seek(io::SeekFrom::End(0)).unwrap();
                        f.write_all(b"APPENDED").unwrap();
                    }
                }
                HookAction::Continue
            }
        }
        for (faults, _) in routes() {
            for same_size in [false, true] {
                let t = TempDir::new().unwrap();
                let src = t.path().join("src");
                let out = t.path().join("out");
                fs::create_dir(&src).unwrap();
                fs::create_dir(&out).unwrap();
                big_source(&src.join("f"));
                let got = copy_atomic_with(
                    &open_regular(&src, "f"),
                    &dir(&out),
                    OsStr::new("f"),
                    faults,
                    &mut Scribble(src.clone(), same_size),
                );
                assert!(
                    matches!(got, Ok(Placed::SourceChanged)),
                    "{faults:?} same_size={same_size}: {got:?}"
                );
                assert!(!out.join("f").exists(), "{faults:?}: published torn bytes");
                assert!(parts_in(&out).is_empty(), "{faults:?}: temp left");
            }
        }
        // Unwritten, the same source is published.
        let t = TempDir::new().unwrap();
        big_source(&t.path().join("f"));
        let out = t.path().join("out");
        fs::create_dir(&out).unwrap();
        let got = copy_atomic(&open_regular(t.path(), "f"), &dir(&out), OsStr::new("f"));
        assert!(matches!(got, Ok(Placed::Done { .. })), "{got:?}");
    }

    #[test]
    fn a_refused_chown_is_counted_and_drops_the_set_id_bits() {
        for (faults, kept, dropped) in [
            (refusing(false, false), true, false),
            (refusing(true, false), false, true),
            (refusing(true, true), false, true),
        ] {
            let (_t, src, out) = setid_fixture(0o6755);
            let setid = setid_kept(&src);
            let mode = if dropped { 0o755 } else { 0o755 | setid };
            let got = copy_atomic_with(
                &open_regular(&src, "f"),
                &dir(&out),
                OsStr::new("f"),
                faults,
                &mut NoHook,
            );
            let placed = match got {
                Ok(placed) => placed,
                Err(Stop::Err(e)) => panic!("{faults:?}: {e}"),
                Err(Stop::Abandoned) => panic!("{faults:?}: abandoned"),
            };
            assert_eq!(placed, Placed::Done { owner_kept: kept }, "{faults:?}");
            let copy = fs::metadata(out.join("f")).unwrap();
            assert_eq!(copy.mode() & 0o7777, mode, "{faults:?}");
            assert_eq!(read(&out.join("f")), b"SETID", "{faults:?}");
        }
    }

    #[test]
    fn a_refused_owner_still_carries_the_group() {
        let (_t, src, out) = setid_fixture(0o640);
        let default_group = {
            put(&out.join("probe"), b"");
            let gid = fs::metadata(out.join("probe")).unwrap().gid();
            fs::remove_file(out.join("probe")).unwrap();
            gid
        };
        let Some(group) = another_group(default_group) else {
            eprintln!("the caller has one group; the carried group is visible where it has two");
            return;
        };
        chgrp(&src.join("f"), group);

        let got = copy_atomic_with(
            &open_regular(&src, "f"),
            &dir(&out),
            OsStr::new("f"),
            refusing(true, false),
            &mut NoHook,
        );
        assert!(
            matches!(got, Ok(Placed::Done { owner_kept: false })),
            "{got:?}"
        );
        assert_eq!(fs::metadata(out.join("f")).unwrap().gid(), group);

        // Refused as well, the group stays whatever the new file was given.
        let again = t_out(&out, "g");
        let got = copy_atomic_with(
            &open_regular(&src, "f"),
            &dir(&again),
            OsStr::new("f"),
            refusing(true, true),
            &mut NoHook,
        );
        assert!(
            matches!(got, Ok(Placed::Done { owner_kept: false })),
            "{got:?}"
        );
        assert_eq!(fs::metadata(again.join("f")).unwrap().gid(), default_group);
    }

    fn t_out(base: &Path, name: &str) -> PathBuf {
        let path = base.join(name);
        fs::create_dir(&path).unwrap();
        path
    }

    /// The temp is observed at each metadata step: the owner first (mode still the temp's
    /// own), then the mode (set-id bits intact, mtime still the copy's), then the times.
    #[test]
    fn metadata_is_set_owner_then_mode_then_times() {
        struct Watch {
            out: PathBuf,
            seen: Vec<(Step, u32, (i64, i64))>,
        }
        impl StepHook for Watch {
            fn at(&mut self, step: Step) -> HookAction {
                if matches!(
                    step,
                    Step::Owned | Step::Moded | Step::Timed | Step::TempSynced
                ) {
                    let temp = parts_in(&self.out).pop().expect("the temp exists");
                    let m = fs::symlink_metadata(self.out.join(temp)).unwrap();
                    self.seen
                        .push((step, m.mode() & 0o7777, (m.mtime(), m.mtime_nsec())));
                }
                HookAction::Continue
            }
        }
        let (_t, src, out) = setid_fixture(0o4755);
        let full = 0o755 | setid_kept(&src);
        let mtime = (1_500_000_000, 123_456_789);
        set_times(&src.join("f"), (1_400_000_000, 7), mtime);
        let mut watch = Watch {
            out: out.clone(),
            seen: Vec::new(),
        };
        let got = copy_atomic_with(
            &open_regular(&src, "f"),
            &dir(&out),
            OsStr::new("f"),
            Faults::default(),
            &mut watch,
        );
        assert!(
            matches!(got, Ok(Placed::Done { owner_kept: true })),
            "{got:?}"
        );
        let steps: Vec<Step> = watch.seen.iter().map(|s| s.0).collect();
        assert_eq!(
            steps,
            [Step::Owned, Step::Moded, Step::Timed, Step::TempSynced]
        );
        let (_, owned_mode, owned_mtime) = watch.seen[0];
        assert_eq!(owned_mode, 0o600, "the mode is set after the owner");
        assert_ne!(owned_mtime, mtime);
        let (_, moded_mode, moded_mtime) = watch.seen[1];
        assert_eq!(moded_mode, full, "chown cleared nothing the mode then set");
        assert_ne!(moded_mtime, mtime, "the times are set after the mode");
        let (_, timed_mode, timed_mtime) = watch.seen[2];
        assert_eq!((timed_mode, timed_mtime), (full, mtime));
        let copy = fs::metadata(out.join("f")).unwrap();
        assert_eq!(copy.mode() & 0o7777, full);
        assert_eq!((copy.mtime(), copy.mtime_nsec()), mtime);
        assert_eq!((copy.atime(), copy.atime_nsec()), (1_400_000_000, 7));
    }

    #[test]
    fn a_non_regular_source_is_never_opened() {
        let t = TempDir::new().unwrap();
        let root = t.path();
        put(&root.join("target"), b"T");
        mkfifo(&root.join("fifo"));
        fs::create_dir(root.join("subdir")).unwrap();
        std::os::unix::fs::symlink(root.join("target"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(root.join("fifo"), root.join("fifo-link")).unwrap();
        let d = dir(root);
        for name in ["fifo", "subdir", "link", "fifo-link"] {
            let got = open_source(&d, OsStr::new(name)).unwrap();
            assert!(matches!(got, Source::NotRegular), "{name}: {got:?}");
        }
        assert!(matches!(
            open_source(&d, OsStr::new("absent")).unwrap(),
            Source::Missing
        ));
        // A FIFO a reader had opened would accept a non-blocking writer; ENXIO proves no one did.
        let c = CString::new(root.join("fifo").as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid C string for the call.
        let w = unsafe { libc::open(c.as_ptr(), libc::O_WRONLY | libc::O_NONBLOCK) };
        assert_eq!(w, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ENXIO));
        assert!(matches!(
            open_source(&d, OsStr::new("target")).unwrap(),
            Source::Regular(_)
        ));
    }

    #[test]
    fn a_symlink_in_the_destination_tree_is_not_followed() {
        let t = TempDir::new().unwrap();
        let into = t.path().join("into");
        let elsewhere = t.path().join("elsewhere");
        fs::create_dir(&into).unwrap();
        fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, into.join("planted")).unwrap();
        put(&into.join("plain"), b"F");

        let base = dir(&into);
        for names in [["planted", "deeper"], ["plain", "deeper"]] {
            let names: Vec<&OsStr> = names.iter().map(OsStr::new).collect();
            let err = walk_create_nofollow(&base, &names).unwrap_err();
            assert!(err.to_string().contains("is not a directory"), "{err}");
        }
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
        assert!(
            fs::symlink_metadata(into.join("planted"))
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let made = walk_create_nofollow(&base, &[OsStr::new("a"), OsStr::new("b")]).unwrap();
        assert!(into.join("a/b").is_dir());
        let src = t.path().join("src");
        fs::create_dir(&src).unwrap();
        put(&src.join("f"), b"BYTES");
        copy_atomic(&open_regular(&src, "f"), &made, OsStr::new("f")).unwrap();
        assert_eq!(read(&into.join("a/b/f")), b"BYTES");
    }

    #[test]
    fn a_symlinked_source_parent_is_not_followed() {
        let t = TempDir::new().unwrap();
        let base = t.path().canonicalize().unwrap();
        fs::create_dir(base.join("real")).unwrap();
        put(&base.join("real/f"), b"F");
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();

        let root = DirHandle::open(Path::new("/")).unwrap();
        let err = walk_nofollow(&root, &components(&base.join("link"))).unwrap_err();
        assert!(is_not_a_directory(&err), "{err}");

        let parent = walk_nofollow(&root, &components(&base.join("real"))).unwrap();
        assert!(matches!(
            open_source(&parent, OsStr::new("f")).unwrap(),
            Source::Regular(_)
        ));
    }

    // ---- Same-filesystem move ----

    fn admit_all(_: &Stat) -> Result<(), &'static str> {
        Ok(())
    }

    fn move_in(base: &Path, name: &str, out: &Path, faults: Faults) -> io::Result<Moved> {
        let (s, d) = (dir(base), dir(out));
        move_same_fs_with(
            &s,
            OsStr::new(name),
            || Ok(&d),
            OsStr::new(name),
            admit_all,
            faults,
            &mut NoHook,
        )
    }

    fn link_route() -> Faults {
        Faults {
            force_link_route: true,
            ..Faults::default()
        }
    }

    /// Plants a file at the destination name after the move found it free and before the
    /// rename, so the rename itself has to refuse.
    struct PlantAt(PathBuf);

    impl StepHook for PlantAt {
        fn at(&mut self, step: Step) -> HookAction {
            if step == Step::Cleared {
                put(&self.0, b"PLANTED");
            }
            HookAction::Continue
        }
    }

    #[test]
    fn a_same_filesystem_move_keeps_the_inode() {
        for faults in [Faults::default(), link_route()] {
            let t = TempDir::new().unwrap();
            let (src, out) = (t.path().join("src"), t.path().join("out"));
            fs::create_dir(&src).unwrap();
            fs::create_dir(&out).unwrap();
            put(&src.join("f"), b"BYTES");
            fs::set_permissions(src.join("f"), fs::Permissions::from_mode(0o640)).unwrap();
            set_times(&src.join("f"), (1_500_000_000, 11), (1_400_000_000, 22));
            let before = fs::symlink_metadata(src.join("f")).unwrap();

            let got = move_in(&src, "f", &out, faults).unwrap();
            assert_eq!(got, Moved::Done, "{faults:?}");
            assert!(fs::symlink_metadata(src.join("f")).is_err(), "{faults:?}");
            let after = fs::symlink_metadata(out.join("f")).unwrap();
            assert_eq!(
                (after.dev(), after.ino(), after.nlink()),
                (before.dev(), before.ino(), 1),
                "{faults:?}"
            );
            assert_eq!(after.mode(), before.mode());
            assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
            assert_eq!(
                (after.mtime(), after.mtime_nsec()),
                (1_400_000_000, 22),
                "{faults:?}"
            );
            assert_eq!(read(&out.join("f")), b"BYTES");
        }
    }

    #[test]
    fn a_same_filesystem_move_never_replaces() {
        // Taken before the move looks.
        let t = TempDir::new().unwrap();
        let (src, out) = (t.path().join("src"), t.path().join("out"));
        fs::create_dir(&src).unwrap();
        fs::create_dir(&out).unwrap();
        put(&src.join("f"), b"NEW");
        put(&out.join("f"), b"OLD");
        assert_eq!(
            move_in(&src, "f", &out, Faults::default()).unwrap(),
            Moved::Existing
        );
        assert_eq!(read(&src.join("f")), b"NEW");
        assert_eq!(read(&out.join("f")), b"OLD");

        // Taken after the move looked and before it renamed: the rename itself refuses.
        put(&src.join("g"), b"NEW");
        let (s, d) = (dir(&src), dir(&out));
        let got = move_same_fs_with(
            &s,
            OsStr::new("g"),
            || Ok(&d),
            OsStr::new("g"),
            admit_all,
            Faults::default(),
            &mut PlantAt(out.join("g")),
        )
        .unwrap();
        assert_eq!(got, Moved::Existing);
        assert_eq!(read(&src.join("g")), b"NEW");
        assert_eq!(read(&out.join("g")), b"PLANTED");
    }

    #[test]
    fn a_same_filesystem_move_by_the_link_route_never_replaces() {
        let t = TempDir::new().unwrap();
        let (src, out) = (t.path().join("src"), t.path().join("out"));
        fs::create_dir(&src).unwrap();
        fs::create_dir(&out).unwrap();
        put(&src.join("f"), b"NEW");
        put(&out.join("f"), b"OLD");
        assert_eq!(
            move_in(&src, "f", &out, link_route()).unwrap(),
            Moved::Existing
        );
        assert_eq!(read(&src.join("f")), b"NEW");
        assert_eq!(read(&out.join("f")), b"OLD");

        put(&src.join("g"), b"NEW");
        let (s, d) = (dir(&src), dir(&out));
        let got = move_same_fs_with(
            &s,
            OsStr::new("g"),
            || Ok(&d),
            OsStr::new("g"),
            admit_all,
            link_route(),
            &mut PlantAt(out.join("g")),
        )
        .unwrap();
        assert_eq!(got, Moved::Existing);
        assert_eq!(read(&src.join("g")), b"NEW");
        assert_eq!(read(&out.join("g")), b"PLANTED");
        assert_eq!(fs::metadata(src.join("g")).unwrap().nlink(), 1);
        assert_eq!(fs::metadata(out.join("g")).unwrap().nlink(), 1);
    }

    /// Without any caller check, only a regular file moves: a directory would carry its
    /// subtree, and a FIFO or a symlink would move as itself. None is touched, the caller's
    /// check never sees them, and the destination directory is never reached.
    #[test]
    fn a_same_filesystem_move_skips_a_non_regular_object() {
        for faults in [Faults::default(), link_route()] {
            let t = TempDir::new().unwrap();
            let (src, out) = (t.path().join("src"), t.path().join("out"));
            fs::create_dir(&src).unwrap();
            fs::create_dir(&out).unwrap();
            put(&src.join("target"), b"T");
            mkfifo(&src.join("fifo"));
            fs::create_dir(src.join("subdir")).unwrap();
            put(&src.join("subdir/inner"), b"I");
            std::os::unix::fs::symlink(src.join("target"), src.join("link")).unwrap();

            let s = dir(&src);
            for name in ["fifo", "subdir", "link"] {
                let mut reached = false;
                let got = move_same_fs_with(
                    &s,
                    OsStr::new(name),
                    || {
                        reached = true;
                        Err(io::Error::other("the destination was reached"))
                    },
                    OsStr::new(name),
                    |_: &Stat| panic!("{name}: the caller's check saw a non-regular object"),
                    faults,
                    &mut NoHook,
                )
                .unwrap();
                assert_eq!(got, Moved::NotRegular, "{name}: {faults:?}");
                assert!(!reached, "{name}");
                assert!(fs::symlink_metadata(src.join(name)).is_ok(), "{name}");
                assert!(fs::symlink_metadata(out.join(name)).is_err(), "{name}");
            }
            assert_eq!(read(&src.join("subdir/inner")), b"I");
            assert!(
                fs::symlink_metadata(src.join("link"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(read(&src.join("target")), b"T");
            assert_eq!(
                move_in(&src, "absent", &out, faults).unwrap(),
                Moved::Missing
            );

            // A regular file the caller's check turns down stays too.
            let d = dir(&out);
            let got = move_same_fs_with(
                &s,
                OsStr::new("target"),
                || Ok(&d),
                OsStr::new("target"),
                |_: &Stat| Err("no longer matches --min-size"),
                faults,
                &mut NoHook,
            )
            .unwrap();
            assert_eq!(got, Moved::Rejected("no longer matches --min-size"));
            assert_eq!(read(&src.join("target")), b"T");
            assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
        }
    }
}
