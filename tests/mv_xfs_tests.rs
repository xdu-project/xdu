//! The cross-filesystem tier: `xdu-mv` driven across a real second filesystem.
//!
//! Every test here is ignored by plain `cargo test`, because it needs a writable directory on
//! another filesystem named by `XDU_TEST_XFS_DIR`. `tests/xfs-wire.sh` runs them with
//! `--include-ignored` (CI does, on a tmpfs on Linux and an APFS disk image on macOS; AGENTS.md,
//! Testing, says how). The fixture never lets a test pass without the real condition: it panics
//! when the variable is unset, and when a probe rename from a temp dir into that directory does
//! not fail with `EXDEV`, which is the very answer that sends `xdu-mv` down its copy path. Device
//! numbers are not compared to decide: they agree across a bind mount and can differ within one
//! filesystem.
//!
//! The source tree and its index live in a temp dir on the default temp filesystem; `--into`
//! is a fresh directory under `XDU_TEST_XFS_DIR`.

mod common;

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use common::{build_index, create_test_file, list_files_recursive, run_mv};

const XFS_DIR: &str = "XDU_TEST_XFS_DIR";

/// A fresh directory on the second filesystem, after proving that a rename into it really
/// crosses filesystems.
fn second_filesystem() -> TempDir {
    let Some(base) = std::env::var_os(XFS_DIR) else {
        panic!("{XFS_DIR} is not set: name a writable directory on another filesystem");
    };
    let base = PathBuf::from(base);
    let fresh = || {
        TempDir::new_in(&base)
            .unwrap_or_else(|e| panic!("{XFS_DIR}={} is not writable: {e}", base.display()))
    };
    let landing = fresh();
    let probe = TempDir::new().unwrap();
    fs::write(probe.path().join("probe"), b"probe").unwrap();
    match fs::rename(probe.path().join("probe"), landing.path().join("probe")) {
        Ok(()) => panic!(
            "{XFS_DIR} is not on another filesystem: a rename from {} into {} succeeded",
            probe.path().display(),
            base.display()
        ),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {}
        Err(e) => panic!("{XFS_DIR}: the probe rename failed with {e}, not EXDEV"),
    }
    fresh()
}

struct Fixture {
    _src: TempDir,
    _into: TempDir,
    source: PathBuf,
    index: PathBuf,
    into: PathBuf,
}

impl Fixture {
    /// A source tree of `files` (relative path, size) on the temp filesystem, indexed, and an
    /// empty `--into` on the second filesystem.
    fn new(files: &[(&str, usize)]) -> Fixture {
        let into_dir = second_filesystem();
        let src_dir = TempDir::new().unwrap();
        let base = src_dir.path().canonicalize().unwrap();
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        for (rel, size) in files {
            create_test_file(&source.join(rel), *size).unwrap();
        }
        let index = base.join("index");
        build_index(&source, &index);
        Fixture {
            into: into_dir.path().canonicalize().unwrap(),
            _src: src_dir,
            _into: into_dir,
            source,
            index,
        }
    }

    fn run(&self, extra: &[&str]) -> (String, String, bool) {
        let mut args = vec![
            "-i",
            self.index.to_str().unwrap(),
            "--into",
            self.into.to_str().unwrap(),
            "--root",
            self.source.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        run_mv(&args)
    }

    fn src(&self, rel: &str) -> PathBuf {
        self.source.join(rel)
    }

    fn dest(&self, rel: &str) -> PathBuf {
        self.into.join(rel)
    }
}

/// Every file under `dir`, relative to it, temps included.
fn files_under(dir: &Path) -> Vec<String> {
    list_files_recursive(dir)
        .iter()
        .map(|p| p.strip_prefix(dir).unwrap().to_str().unwrap().to_string())
        .collect()
}

#[allow(clippy::unnecessary_cast)]
fn set_times(path: &Path, atime: (i64, i64), mtime: (i64, i64)) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
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
    assert_eq!(ret, 0, "utimensat {}", path.display());
}

#[test]
#[ignore = "needs XDU_TEST_XFS_DIR on a second filesystem"]
fn mv_across_filesystems_carries_bytes_mode_and_times() {
    let f = Fixture::new(&[("u/a.dat", 3000), ("u/deep/b.dat", 70_000)]);
    let expected_a = fs::read(f.src("u/a.dat")).unwrap();
    let expected_b = fs::read(f.src("u/deep/b.dat")).unwrap();
    fs::set_permissions(f.src("u/a.dat"), fs::Permissions::from_mode(0o640)).unwrap();
    fs::set_permissions(f.src("u/deep/b.dat"), fs::Permissions::from_mode(0o604)).unwrap();
    set_times(
        &f.src("u/a.dat"),
        (1_400_000_000, 7),
        (1_500_000_000, 123_456_789),
    );
    set_times(
        &f.src("u/deep/b.dat"),
        (1_410_000_000, 0),
        (1_510_000_000, 5),
    );

    let (out, err, ok) = f.run(&["--force", "-v"]);
    assert!(ok, "xdu-mv failed:\n{out}\n{err}");
    assert!(out.contains("Moved: 2"), "{out}");
    assert!(!out.contains("Failed"), "{out}\n{err}");

    for (rel, mode, atime, mtime) in [
        (
            "u/a.dat",
            0o640,
            (1_400_000_000, 7),
            (1_500_000_000, 123_456_789),
        ),
        (
            "u/deep/b.dat",
            0o604,
            (1_410_000_000, 0),
            (1_510_000_000, 5),
        ),
    ] {
        assert!(
            fs::symlink_metadata(f.src(rel)).is_err(),
            "{rel}: the source is still there"
        );
        // Metadata before bytes: reading the copy can move its atime.
        let m = fs::symlink_metadata(f.dest(rel)).unwrap();
        assert!(m.file_type().is_file(), "{rel}");
        assert_eq!(m.mode() & 0o7777, mode, "{rel}: mode");
        assert_eq!((m.mtime(), m.mtime_nsec()), mtime, "{rel}: mtime");
        assert_eq!((m.atime(), m.atime_nsec()), atime, "{rel}: atime");
    }
    assert_eq!(fs::read(f.dest("u/a.dat")).unwrap(), expected_a);
    assert_eq!(fs::read(f.dest("u/deep/b.dat")).unwrap(), expected_b);
    assert_eq!(files_under(&f.into), ["u/a.dat", "u/deep/b.dat"]);
}

#[test]
#[ignore = "needs XDU_TEST_XFS_DIR on a second filesystem"]
fn mv_across_filesystems_skips_an_existing_destination() {
    let f = Fixture::new(&[("u/a.dat", 100), ("u/b.dat", 200)]);
    let expected_b = fs::read(f.src("u/b.dat")).unwrap();
    fs::create_dir_all(f.dest("u")).unwrap();
    fs::write(f.dest("u/a.dat"), b"ALREADY HERE").unwrap();

    let (out, err, ok) = f.run(&["--force", "-v"]);
    assert!(
        !ok,
        "an existing destination must exit non-zero:\n{out}\n{err}"
    );
    assert!(out.contains("Moved: 1"), "{out}");
    assert!(out.contains("Existing at destination: 1"), "{out}");
    assert_eq!(fs::read(f.dest("u/a.dat")).unwrap(), b"ALREADY HERE");
    assert_eq!(
        fs::read(f.src("u/a.dat")).unwrap().len(),
        100,
        "the source of the taken name stays"
    );
    assert!(fs::symlink_metadata(f.src("u/b.dat")).is_err());
    assert_eq!(fs::read(f.dest("u/b.dat")).unwrap(), expected_b);
    assert_eq!(
        files_under(&f.into),
        ["u/a.dat", "u/b.dat"],
        "a temp was left"
    );
}

#[test]
#[ignore = "needs XDU_TEST_XFS_DIR on a second filesystem"]
fn mv_across_filesystems_removes_the_source_and_lands_on_another_device() {
    let f = Fixture::new(&[("u/a.dat", 4096)]);
    let source_dev = fs::metadata(f.src("u")).unwrap().dev();

    let (out, err, ok) = f.run(&["--force"]);
    assert!(ok, "xdu-mv failed:\n{out}\n{err}");
    assert!(out.contains("Moved: 1"), "{out}");
    assert!(fs::symlink_metadata(f.src("u/a.dat")).is_err());
    assert_eq!(files_under(&f.source), Vec::<String>::new());
    let landed = fs::metadata(f.dest("u/a.dat")).unwrap();
    assert_eq!(landed.len(), 4096);
    assert_eq!(landed.nlink(), 1);
    assert_ne!(
        landed.dev(),
        source_dev,
        "the copy is on the source's device"
    );
}
