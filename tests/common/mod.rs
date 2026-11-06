//! Shared helpers for the real-binary integration tests.
//!
//! These drive the actual `xdu` / `xdu-find` binaries (`std::process::Command`) against
//! throwaway `tempfile` indexes — never a real filesystem — and assert concrete
//! post-conditions (row counts, files present-or-absent), not merely exit 0.
//!
//! `dead_code` is allowed because each test file uses only a subset of these helpers,
//! and every `tests/*.rs` file compiles the module into its own separate crate.
#![allow(dead_code)]

pub mod s3;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use duckdb::Connection;
use xdu::view_query::{self, DirEntry};
use xdu::{IndexLocator, QueryFilters, SortMode};

/// Path to a binary built for *this* test invocation.
///
/// `CARGO_BIN_EXE_<name>` is set by Cargo to the binary compiled for the current test
/// profile, so the tests never accidentally exercise a stale `target/release` artifact
/// left over from an earlier build.
pub fn binary_path(name: &str) -> PathBuf {
    match name {
        "xdu" => env!("CARGO_BIN_EXE_xdu").into(),
        "xdu-find" => env!("CARGO_BIN_EXE_xdu-find").into(),
        "xdu-rm" => env!("CARGO_BIN_EXE_xdu-rm").into(),
        "xdu-view" => env!("CARGO_BIN_EXE_xdu-view").into(),
        "xdu-cp" => env!("CARGO_BIN_EXE_xdu-cp").into(),
        "xdu-mv" => env!("CARGO_BIN_EXE_xdu-mv").into(),
        other => panic!("unknown test binary: {other}"),
    }
}

/// Create a test file of a specific byte size, making parent dirs as needed.
pub fn create_test_file(path: &Path, size: usize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    file.write_all(&vec![b'x'; size])?;
    Ok(())
}

/// Set the access time of a file to a specific number of days ago.
///
/// Lives here rather than in a single test file: an integration fixture kept private to
/// one `tests/*.rs` is how that file ends up re-declaring helpers this module already
/// provides, which is exactly how a defective local `binary_path` survived here.
pub fn set_atime_days_ago(path: &Path, days: u64) -> std::io::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old_time = now - (days * 86400);

    // Get current mtime to preserve it
    let metadata = fs::metadata(path)?;
    let mtime = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Use libc to set atime while preserving mtime
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: old_time as i64,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: mtime as i64,
            tv_nsec: 0,
        },
    ];
    unsafe {
        if libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Set the access time of a file to `secs` since the epoch, preserving its mtime.
///
/// The absolute form of `set_atime_days_ago`: two trees stamped from one epoch carry equal
/// atimes, where two `now()` calls could straddle a second boundary.
pub fn set_atime_at(path: &Path, secs: u64) -> std::io::Result<()> {
    let mtime = fs::metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: secs as i64,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: mtime as i64,
            tv_nsec: 0,
        },
    ];
    unsafe {
        if libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// One partition's subtree for the `xdu-view` listing tests: each file's path inside the
/// partition, its ASCII twin's path, its size, and its age in days.
///
/// The non-ASCII names cover a non-ASCII leaf, a decomposed and a composed accent in one
/// directory name two levels up from a file, U+FFFD as a lossy crawl stores it, and a 4-byte
/// code point. `alpha/z` is shorter than any miscounted offset, so a wrong cut drops it. No name
/// holds `_` or `%`, which `LIKE` reads as wildcards.
pub const VIEW_TREE: &[(&str, &str, usize, u64)] = &[
    ("alpha/r\u{e9}sum\u{e9}.txt", "alpha/resume.txt", 101, 1),
    ("alpha/z", "alpha/z", 202, 2),
    ("beta/f", "beta/f", 303, 3),
    ("top", "top", 404, 4),
    ("e\u{301}t\u{e9}/a/deep.txt", "ete/a/deep.txt", 505, 5),
    ("bad\u{FFFD}name/b", "badxname/b", 606, 6),
    ("clef\u{1D11E}/c", "clefg/c", 707, 7),
];

/// Create `VIEW_TREE` (or its ASCII twin) under `root/partition`, every file's atime set from
/// `epoch`; returns each file's path relative to `root` with its size.
pub fn view_tree(root: &Path, partition: &str, ascii: bool, epoch: u64) -> Vec<(String, usize)> {
    VIEW_TREE
        .iter()
        .map(|&(name, twin, size, days)| {
            let rel = format!("{partition}/{}", if ascii { twin } else { name });
            let file = root.join(&rel);
            create_test_file(&file, size).unwrap();
            set_atime_at(&file, epoch - days * 86400).unwrap();
            (rel, size)
        })
        .collect()
}

/// The names directly under each directory holding `files`, keyed by the directory's path
/// under `base`; `files` are relative to `base`, as `view_tree` returns them. Read from the
/// fixture's own paths, so an expectation never repeats the cut under test.
pub fn view_tree_children(
    base: &str,
    files: &[(String, usize)],
) -> BTreeMap<String, BTreeSet<String>> {
    let mut children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (rel, _) in files {
        let parts: Vec<&str> = rel.split('/').collect();
        for depth in 1..parts.len() {
            let dir = format!("{base}/{}", parts[..depth].join("/"));
            children
                .entry(dir)
                .or_default()
                .insert(parts[depth].to_string());
        }
    }
    children
}

/// What one walk of a view saw: every listing in the order made, and every file reached
/// with its size.
pub struct ViewWalk {
    pub listings: Vec<(String, Vec<DirEntry>)>,
    pub files: BTreeMap<String, i64>,
}

/// Walk `view` ("list" or "tree") of `partition` from its root down, entering every listed
/// directory by its `path`, and assert each listing's names equal `expected`'s for that
/// directory, with the message `"{arm} {view} full names under {dir}"`. Every directory
/// walked must be non-ASCII, so the walk cannot quietly degrade to an ASCII tree.
pub fn walk_view(
    conn: &Connection,
    index: &IndexLocator,
    partition: &str,
    arm: &str,
    view: &str,
    expected: &BTreeMap<String, BTreeSet<String>>,
) -> ViewWalk {
    let root = view_query::partition_root(conn, index, partition)
        .unwrap_or_else(|e| panic!("{arm} root of {partition}: {e:#}"));
    let mut walk = ViewWalk {
        listings: Vec::new(),
        files: BTreeMap::new(),
    };
    let mut pending = VecDeque::from([root]);
    while let Some(dir) = pending.pop_front() {
        assert!(
            !dir.is_ascii(),
            "{arm} {view} walked an ASCII directory {dir}"
        );
        let filters = QueryFilters::new();
        let entries = match view {
            "list" => {
                view_query::list_directory(conn, index, partition, &dir, &filters, SortMode::Name)
            }
            "tree" => {
                view_query::tree_directory(conn, index, partition, &dir, &filters, SortMode::Name)
            }
            other => panic!("unknown view {other}"),
        }
        .unwrap_or_else(|e| panic!("{arm} {view} of {dir}: {e:#}"));
        let mut names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        names.sort();
        let want: Vec<String> = expected
            .get(&dir)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default();
        assert_eq!(names, want, "{arm} {view} full names under {dir}");
        for entry in &entries {
            if entry.is_dir {
                pending.push_back(entry.path.clone());
            } else {
                walk.files.insert(entry.path.clone(), entry.total_size);
            }
        }
        walk.listings.push((dir, entries));
    }
    walk
}

/// Set the modification time of a file to a specific number of days ago.
///
/// The mtime mirror of `set_atime_days_ago`: the atime is preserved while the
/// mtime moves, so `--mtime-older-than` fixtures do not disturb the atime ones.
pub fn set_mtime_days_ago(path: &Path, days: u64) -> std::io::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old_time = now - (days * 86400);

    // Get current atime to preserve it
    let metadata = fs::metadata(path)?;
    let atime = metadata
        .accessed()?
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Use libc to set mtime while preserving atime
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: atime as i64,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: old_time as i64,
            tv_nsec: 0,
        },
    ];
    unsafe {
        if libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Run `xdu` with arbitrary args; returns (stdout, stderr, success).
pub fn run_xdu(args: &[&str]) -> (String, String, bool) {
    let output = Command::new(binary_path("xdu"))
        .args(args)
        .output()
        .expect("failed to spawn xdu");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Build an index over `source` into `index`, asserting success.
///
/// Uses `--apparent-size` so sizes are exact and filesystem-independent.
pub fn build_index(source: &Path, index: &Path) {
    let (_out, err, ok) = run_xdu(&[
        "--apparent-size",
        "-o",
        index.to_str().unwrap(),
        source.to_str().unwrap(),
    ]);
    assert!(ok, "xdu build failed: {err}");
}

/// Run `xdu-find` with arbitrary args; returns (stdout, stderr, success).
pub fn run_find(args: &[&str]) -> (String, String, bool) {
    let output = Command::new(binary_path("xdu-find"))
        .args(args)
        .output()
        .expect("failed to spawn xdu-find");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Run `xdu-rm` with arbitrary args; returns (stdout, stderr, success).
pub fn run_rm(args: &[&str]) -> (String, String, bool) {
    let output = Command::new(binary_path("xdu-rm"))
        .args(args)
        .output()
        .expect("failed to spawn xdu-rm");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Run `xdu-cp` with arbitrary args and no input; returns (stdout, stderr, success).
pub fn run_cp(args: &[&str]) -> (String, String, bool) {
    run_with_stdin("xdu-cp", args, None)
}

/// Run `xdu-mv` with arbitrary args and no input; returns (stdout, stderr, success).
pub fn run_mv(args: &[&str]) -> (String, String, bool) {
    run_with_stdin("xdu-mv", args, None)
}

/// Run a project binary with `input` on stdin, or with stdin closed when `input` is `None`;
/// returns (stdout, stderr, success).
///
/// The prompt tests need both: an answer, and end of input with nothing typed.
pub fn run_with_stdin(name: &str, args: &[&str], input: Option<&str>) -> (String, String, bool) {
    use std::process::Stdio;
    let mut child = Command::new(binary_path(name))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {name}: {e}"));
    {
        let mut stdin = child.stdin.take().expect("piped stdin");
        if let Some(text) = input {
            let _ = stdin.write_all(text.as_bytes());
        }
    }
    let output = child.wait_with_output().expect("wait for child");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Run `xdu-view` with arbitrary args; returns (stdout, stderr, success).
///
/// Only the pre-terminal paths are drivable without a TTY: a refusal exits before the
/// terminal is touched, while an accepted index proceeds into TUI setup and fails
/// without one.
pub fn run_view(args: &[&str]) -> (String, String, bool) {
    let output = Command::new(binary_path("xdu-view"))
        .args(args)
        .output()
        .expect("failed to spawn xdu-view");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Run a project binary with `HOME` redirected; returns (stdout, stderr, success).
///
/// `HOME` is the lever for DuckDB's extension cache (`$HOME/.duckdb/extensions`), so
/// pointing it at an empty directory makes a runtime extension download observable.
pub fn run_binary_with_home(name: &str, home: &Path, args: &[&str]) -> (String, String, bool) {
    let output = Command::new(binary_path(name))
        .args(args)
        .env("HOME", home)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {name}: {e}"));
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Every regular file beneath `dir`, recursively, sorted.
///
/// Returns the paths rather than a count so an assertion can report *what* was written,
/// not merely that something was.
pub fn list_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(list_files_recursive(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Query the index row count, optionally scoped by extra `xdu-find` args.
pub fn find_count(index: &Path, extra: &[&str]) -> i64 {
    let mut args: Vec<&str> = vec!["-i", index.to_str().unwrap(), "--count"];
    args.extend_from_slice(extra);
    let (out, err, ok) = run_find(&args);
    assert!(ok, "xdu-find --count failed: {err}");
    out.trim()
        .parse()
        .unwrap_or_else(|_| panic!("non-integer count output: {out:?}"))
}

/// Sorted list of indexed paths (via `xdu-find -f path`).
pub fn find_paths(index: &Path) -> Vec<String> {
    let (out, err, ok) = run_find(&["-i", index.to_str().unwrap(), "-f", "path"]);
    assert!(ok, "xdu-find -f path failed: {err}");
    let mut lines: Vec<String> = out.lines().map(|l| l.to_string()).collect();
    lines.sort();
    lines
}

/// Count leftover `*.partial` files anywhere under the index directory.
pub fn count_partials(index: &Path) -> usize {
    let mut count = 0;
    let entries = match fs::read_dir(index) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    for part in entries.filter_map(|e| e.ok()) {
        let p = part.path();
        if p.is_dir()
            && let Ok(chunks) = fs::read_dir(&p)
        {
            for chunk in chunks.filter_map(|e| e.ok()) {
                if chunk.file_name().to_string_lossy().ends_with(".partial") {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Count finalized `*.parquet` chunk files inside one partition dir of the index.
pub fn count_chunks(index: &Path, partition: &str) -> usize {
    let part_dir = index.join(partition);
    let entries = match fs::read_dir(&part_dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".parquet"))
        .count()
}
