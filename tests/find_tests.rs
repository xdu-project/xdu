//! Integration tests for the `xdu-find` owner/group/mode/mtime filters and the
//! widened csv/json output.
//!
//! These drive the **real** `xdu` / `xdu-find` binaries against throwaway
//! `tempfile` trees and assert concrete post-conditions (row counts, headers,
//! keys, exit status) — never by reimplementing the query. Expectations about
//! the current user and group come from `libc` directly, not from the
//! `xdu::resolve_user` helpers under test, so the tests do not grade the
//! library against itself.

mod common;

use std::ffi::{CStr, CString};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::TempDir;

use common::{
    build_index, create_test_file, find_count, run_find, set_atime_at, set_mtime_days_ago,
};

/// Current login name and uid from the system database, independent of `xdu`.
fn current_user() -> (String, u32) {
    unsafe {
        let euid = libc::geteuid();
        let entry = libc::getpwuid(euid);
        assert!(!entry.is_null(), "no passwd entry for euid {euid}");
        let name = CStr::from_ptr((*entry).pw_name)
            .to_string_lossy()
            .into_owned();
        (name, euid)
    }
}

/// Current group name and gid from the system database, independent of `xdu`.
fn current_group() -> (String, u32) {
    unsafe {
        let egid = libc::getegid();
        let entry = libc::getgrgid(egid);
        assert!(!entry.is_null(), "no group entry for egid {egid}");
        let name = CStr::from_ptr((*entry).gr_name)
            .to_string_lossy()
            .into_owned();
        (name, egid)
    }
}

/// Three-file fixture tree; every file belongs to the invoking user and group.
fn three_file_tree(source: &std::path::Path) {
    create_test_file(&source.join("part/a.txt"), 100).unwrap();
    create_test_file(&source.join("part/b.txt"), 200).unwrap();
    create_test_file(&source.join("part/c.txt"), 300).unwrap();
}

// =============================================================================
// --owner / --group
// =============================================================================

#[test]
fn test_find_owner_filter_current_user_by_name_and_id() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    build_index(&source, &index);

    let (name, uid) = current_user();
    assert_eq!(find_count(&index, &["--owner", &name]), 3);
    assert_eq!(find_count(&index, &["--owner", &uid.to_string()]), 3);
    // Negative control: every file belongs to the invoker, so an unrelated uid
    // must match nothing. Without it an ignored --owner still counts 3.
    assert_eq!(find_count(&index, &["--owner", "4294967294"]), 0);
}

#[test]
fn test_find_owner_filter_unresolvable_name_fails_clean() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    build_index(&source, &index);

    let (out, err, ok) = run_find(&[
        "-i",
        index.to_str().unwrap(),
        "--owner",
        "xdu-no-such-user",
        "--count",
    ]);
    assert!(!ok, "unresolvable owner must exit non-zero");
    assert!(out.is_empty(), "no rows may print, got: {out:?}");
    assert!(
        err.contains("xdu-no-such-user"),
        "stderr names the miss: {err:?}"
    );
}

#[test]
fn test_find_owner_filter_other_user() {
    // Needs privilege: handing a file to another uid fails for an unprivileged
    // invoker, and there is nothing to assert then.
    let nobody = unsafe {
        let cname = CString::new("nobody").unwrap();
        let entry = libc::getpwnam(cname.as_ptr());
        if entry.is_null() {
            eprintln!("SKIP test_find_owner_filter_other_user: no 'nobody' entry here");
            return;
        }
        (*entry).pw_uid
    };
    let (my_name, my_uid) = current_user();
    if nobody == my_uid {
        eprintln!("SKIP test_find_owner_filter_other_user: invoking user is nobody");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    let foreign = source.join("part/c.txt");
    let c_path = CString::new(foreign.to_str().unwrap()).unwrap();
    if unsafe { libc::chown(c_path.as_ptr(), nobody, u32::MAX) } != 0 {
        eprintln!("SKIP test_find_owner_filter_other_user: chown needs privilege here");
        return;
    }
    build_index(&source, &index);

    assert_eq!(find_count(&index, &["--owner", &my_name]), 2);
    assert_eq!(find_count(&index, &["--owner", "nobody"]), 1);
}

#[test]
fn test_find_group_filter_current_group() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    build_index(&source, &index);

    let (name, gid) = current_group();
    assert_eq!(find_count(&index, &["--group", &name]), 3);
    assert_eq!(find_count(&index, &["--group", &gid.to_string()]), 3);
    // Negative control, as for --owner: an unrelated gid must match nothing.
    assert_eq!(find_count(&index, &["--group", "4294967294"]), 0);

    let (out, _err, ok) = run_find(&[
        "-i",
        index.to_str().unwrap(),
        "--group",
        "xdu-no-such-group",
        "--count",
    ]);
    assert!(!ok, "unresolvable group must exit non-zero");
    assert!(out.is_empty(), "no rows may print, got: {out:?}");
}

// =============================================================================
// --mode
// =============================================================================

#[test]
fn test_find_mode_filter_exact_any_all() {
    use std::fs;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    fs::set_permissions(source.join("part/a.txt"), fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(source.join("part/b.txt"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(source.join("part/c.txt"), fs::Permissions::from_mode(0o777)).unwrap();
    build_index(&source, &index);

    assert_eq!(find_count(&index, &["--mode", "644"]), 1);
    assert_eq!(find_count(&index, &["--mode", "/002"]), 1);
    assert_eq!(find_count(&index, &["--mode", "/400"]), 3);
    assert_eq!(find_count(&index, &["--mode", "&4000"]), 0);

    let (out, _err, ok) = run_find(&["-i", index.to_str().unwrap(), "--mode", "999", "--count"]);
    assert!(!ok, "invalid mode SPEC must exit non-zero");
    assert!(out.is_empty(), "no rows may print, got: {out:?}");
}

// =============================================================================
// --mtime-older-than / --mtime-newer-than
// =============================================================================

#[test]
fn test_find_mtime_filters() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    // A day of margin either side of the 30-day threshold: the stored mtime and
    // the query threshold are stamped seconds apart, so an exact 30-day fixture
    // flakes whenever both land in the same Unix second.
    set_mtime_days_ago(&source.join("part/c.txt"), 31).unwrap();
    build_index(&source, &index);

    assert_eq!(find_count(&index, &["--mtime-older-than", "30"]), 1);
    assert_eq!(find_count(&index, &["--mtime-newer-than", "30"]), 2);
    assert_eq!(find_count(&index, &["--mtime-newer-than", "1"]), 2);
}

// =============================================================================
// csv / json carry the new columns; path format is untouched
// =============================================================================

#[test]
fn test_find_csv_carries_new_columns() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    build_index(&source, &index);

    let (out, err, ok) = run_find(&["-i", index.to_str().unwrap(), "-f", "csv"]);
    assert!(ok, "csv query failed: {err}");
    let mut lines = out.lines();
    assert_eq!(
        lines.next().unwrap(),
        "path,size,uid,gid,mode,atime,mtime,ctime"
    );
    let rows: Vec<&str> = lines.collect();
    assert_eq!(rows.len(), 3, "three data rows expected: {rows:?}");
    for row in rows {
        assert_eq!(row.split(',').count(), 8, "eight fields: {row:?}");
    }
}

#[test]
fn test_find_json_carries_new_columns() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    build_index(&source, &index);

    let (out, err, ok) = run_find(&["-i", index.to_str().unwrap(), "-f", "json"]);
    assert!(ok, "json query failed: {err}");
    assert_eq!(out.matches("\"path\":\"").count(), 3);
    for key in [
        "\"uid\":",
        "\"gid\":",
        "\"mode\":",
        "\"mtime\":",
        "\"ctime\":",
    ] {
        assert_eq!(out.matches(key).count(), 3, "key {key} on every row");
    }
}

#[test]
fn test_find_path_format_unchanged() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let index = tmp.path().join("index");
    three_file_tree(&source);
    build_index(&source, &index);

    let (out, err, ok) = run_find(&["-i", index.to_str().unwrap(), "-f", "path"]);
    assert!(ok, "path query failed: {err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(
        lines.iter().all(|l| l.contains("part/")),
        "one path per line, no header: {lines:?}"
    );
}

// =============================================================================
// A key=value directory name reads like any other
// =============================================================================
//
// DuckDB's Hive detection turns a `key=value` directory name that every globbed path carries
// into a column `key` that shadows the row's own. Each case below compares an index whose
// partition or index directory has such a name against a bare twin built from the same files.
// The fixtures stay in this file rather than `tests/common/mod.rs`, which these tests must not
// change: they have to build on a release base that lacks that file's later helpers.

/// The one partition of the partition layout, with apparent sizes. Distinct sizes and atimes
/// give `-f size` and `-f atime` a total order.
const PARTITION_FILES: &[(&str, usize)] = &[
    ("big.bin", 5_000_000),
    ("small.txt", 1),
    ("sub/mid.txt", 300),
];

/// The tree of the index-directory layout: two partitions and a loose file (`__root__`), with
/// distinct sizes, atimes and per-partition file counts, so every ordered output has a total
/// order.
const INDEX_DIR_FILES: &[(&str, usize)] = &[
    ("alpha/a.txt", 100),
    ("alpha/b.bin", 2000),
    ("alpha/deep/c.txt", 300),
    ("beta/d.txt", 400),
    ("beta/e.dat", 5000),
    ("loose.txt", 50),
];

/// Every key an index directory is named after: the eight columns, the `partition` alias the
/// views and `--top` group by, and a key naming no column.
const KEYS: &[&str] = &[
    "path",
    "size",
    "uid",
    "gid",
    "mode",
    "atime",
    "mtime",
    "ctime",
    "partition",
    "k",
];

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Write `files` under `root`, the i-th accessed `i + 1` minutes before `epoch`.
fn write_tree(root: &Path, files: &[(&str, usize)], epoch: u64) {
    for (i, (rel, size)) in files.iter().enumerate() {
        let file = root.join(rel);
        create_test_file(&file, *size).unwrap();
        set_atime_at(&file, epoch - 60 * (i as u64 + 1)).unwrap();
    }
}

/// Index `tree` into `index`, making the directories above it first.
fn build_index_at(tree: &Path, index: &Path) {
    std::fs::create_dir_all(index.parent().unwrap()).unwrap();
    build_index(tree, index);
}

/// The partition layout: a source whose only top-level directory is `size=0`, indexed to the
/// first path returned, and its bare twin, indexed to the second from the same files while
/// that directory was still named `size-0`. A rename keeps every file's inode, so the twin's
/// rows differ from the index's in that one name. The string is the canonical source root.
fn key_value_partition(work: &Path) -> (String, PathBuf, PathBuf) {
    let source = work.join("source");
    write_tree(&source.join("size-0"), PARTITION_FILES, now_secs());
    let twin = work.join("twin");
    build_index(&source, &twin);
    std::fs::rename(source.join("size-0"), source.join("size=0")).unwrap();
    let index = work.join("index");
    build_index(&source, &index);
    let root = source.canonicalize().unwrap().to_str().unwrap().to_string();
    (root, index, twin)
}

/// Output lines in a comparable order: as printed when the flags give a total order (`-f size`,
/// `-f atime`, `--top` over distinct values), sorted otherwise, since two crawls of one tree
/// need not store its rows in the same order.
fn comparable(flags: &[&str], out: &str) -> Vec<String> {
    let mut lines: Vec<String> = out.lines().map(String::from).collect();
    if !flags
        .iter()
        .any(|flag| matches!(*flag, "size" | "atime" | "--top"))
    {
        lines.sort();
    }
    lines
}

/// A partition named `size=0` reads as a name: its files keep their sizes for every filter,
/// count, sort and grouping, with and without `-u`.
#[test]
fn find_reads_a_key_value_partition_as_a_bare_name() {
    let work = TempDir::new().unwrap();
    let (root, index, twin) = key_value_partition(work.path());
    let (index, twin) = (index.to_str().unwrap(), twin.to_str().unwrap());
    let small = vec![
        format!("{root}/size=0/small.txt"),
        format!("{root}/size=0/sub/mid.txt"),
    ];

    let (out, err, ok) = run_find(&["-i", index, "-u", "size=0", "--max-size", "1K"]);
    assert!(
        ok && comparable(&[], &out) == small,
        "key=value partition: -u size=0 --max-size 1K listed {out:?}, not {small:?} (stderr: {err})"
    );

    let (out, err, ok) = run_find(&["-i", index, "--max-size", "1K"]);
    assert!(ok, "{err}");
    assert_eq!(comparable(&[], &out), small, "--max-size 1K without -u");
    let (out, err, ok) = run_find(&["-i", index, "--min-size", "1M"]);
    assert!(ok, "{err}");
    assert_eq!(out, format!("{root}/size=0/big.bin\n"), "--min-size 1M");
    let (out, err, ok) = run_find(&["-i", index, "-f", "size"]);
    assert!(ok, "{err}");
    assert_eq!(
        out.lines().next(),
        Some(format!("5000000\t{root}/size=0/big.bin").as_str()),
        "-f size puts big.bin first: {out:?}"
    );

    // The twin's output under the index's name: its rows' paths, and `--top`'s partition.
    let (bare_dir, kv_dir) = (format!("{root}/size-0/"), format!("{root}/size=0/"));
    let to_index = |out: &str| -> String {
        out.lines()
            .map(|line| match line {
                "size-0" => "size=0\n".to_string(),
                _ => format!("{}\n", line.replace(&bare_dir, &kv_dir)),
            })
            .collect()
    };
    let flag_sets: &[&[&str]] = &[
        &[],
        &["--count"],
        &["--top", "5"],
        &["-f", "csv"],
        &["-f", "size"],
        &["-f", "atime"],
        &["--max-size", "1K"],
        &["--min-size", "1M"],
    ];
    let mut compared = 0;
    for scope in [None, Some(("size=0", "size-0"))] {
        for flags in flag_sets {
            let mut args = vec!["-i", index];
            let mut twin_args = vec!["-i", twin];
            if let Some((name, twin_name)) = scope {
                args.extend(["-u", name]);
                twin_args.extend(["-u", twin_name]);
            }
            args.extend_from_slice(flags);
            twin_args.extend_from_slice(flags);
            let (out, err, ok) = run_find(&args);
            assert!(ok, "{args:?}: {err}");
            let (twin_out, err, ok) = run_find(&twin_args);
            assert!(ok, "{twin_args:?}: {err}");
            assert!(!twin_out.is_empty(), "{twin_args:?} printed nothing");
            assert_eq!(
                comparable(flags, &out),
                comparable(flags, &to_index(&twin_out)),
                "{args:?} against the bare twin"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 2 * flag_sets.len());
}

/// An index stored under a `key=value` directory reads as one stored under a bare name, for
/// every column the key could shadow and for every filter, count, sort and grouping.
#[test]
fn find_reads_an_index_under_a_key_value_directory_as_a_bare_one() {
    let work = TempDir::new().unwrap();
    let tree = work.path().join("tree");
    write_tree(&tree, INDEX_DIR_FILES, now_secs());
    let bare = work.path().join("bare/index");
    build_index_at(&tree, &bare);
    let keyed: Vec<PathBuf> = KEYS
        .iter()
        .map(|key| {
            let index = work.path().join(format!("{key}=0/index"));
            build_index_at(&tree, &index);
            index
        })
        .collect();

    let mtime = work.path().join("mtime=0/index");
    let (out, err, ok) = run_find(&["-i", mtime.to_str().unwrap(), "--mtime-older-than", "3650"]);
    assert!(
        ok && out.is_empty(),
        "key=value index directory: under mtime=0, --mtime-older-than 3650 matched files \
         written seconds ago: {out:?} (stderr: {err})"
    );

    let meta = std::fs::metadata(tree.join("loose.txt")).unwrap();
    let (uid, gid) = (meta.uid().to_string(), meta.gid().to_string());
    let flag_sets: Vec<Vec<&str>> = vec![
        vec![],
        vec!["--count"],
        vec!["--top", "5"],
        vec!["-f", "csv"],
        vec!["-f", "size"],
        vec!["-f", "atime"],
        vec!["-u", "alpha"],
        vec!["-p", "*.txt"],
        vec!["--regex", "-p", "txt$"],
        vec!["--min-size", "1"],
        vec!["--max-size", "1K"],
        vec!["--older-than", "3650"],
        vec!["--newer-than", "1"],
        vec!["--mtime-older-than", "3650"],
        vec!["--mtime-newer-than", "1"],
        vec!["--owner", &uid],
        vec!["--group", &gid],
        vec!["--mode", "/400"],
    ];
    let mut compared = 0;
    for flags in &flag_sets {
        let mut args = vec!["-i", bare.to_str().unwrap()];
        args.extend_from_slice(flags);
        let (want, err, ok) = run_find(&args);
        assert!(ok, "{args:?}: {err}");
        // The fixture is minutes old, so only the two 3650-day filters select nothing.
        let empty = matches!(
            flags.first(),
            Some(&"--older-than") | Some(&"--mtime-older-than")
        );
        assert_eq!(
            want.is_empty(),
            empty,
            "{args:?} on the bare index: {want:?}"
        );
        for (key, index) in KEYS.iter().zip(&keyed) {
            let mut args = vec!["-i", index.to_str().unwrap()];
            args.extend_from_slice(flags);
            let (out, err, ok) = run_find(&args);
            assert!(ok, "under {key}=0, {flags:?}: {err}");
            assert_eq!(
                comparable(flags, &out),
                comparable(flags, &want),
                "under {key}=0, {flags:?} against the bare index"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, flag_sets.len() * KEYS.len());
}
