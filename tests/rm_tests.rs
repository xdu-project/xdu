//! Integration tests for xdu-rm functionality.
//!
//! These tests create temporary directory structures, build indexes using xdu,
//! and verify that xdu-rm correctly deletes files based on query criteria.
//!
//! Binary resolution and every fixture come from `tests/common`, so each case drives the
//! artifact Cargo built for the profile under test rather than whatever happens to be
//! sitting in `target/release`.

mod common;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::TempDir;

use common::{build_index, create_test_file, run_rm, set_atime_at, set_atime_days_ago};

// =============================================================================
// Test: Basic dry-run shows files without deleting
// =============================================================================

#[test]
fn test_dry_run_lists_files_without_deleting() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with files
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/file1.txt"), 100).unwrap();
    create_test_file(&source.join("user1/file2.txt"), 200).unwrap();

    // Build index
    build_index(&source, &index);

    // Run dry-run
    let (stdout, _stderr, success) = run_rm(&["-i", index.to_str().unwrap(), "--dry-run"]);

    assert!(success);
    assert!(stdout.contains("file1.txt"));
    assert!(stdout.contains("file2.txt"));
    assert!(stdout.contains("2 file(s) would be deleted"));

    // Verify files still exist
    assert!(source.join("user1/file1.txt").exists());
    assert!(source.join("user1/file2.txt").exists());
}

// =============================================================================
// Test: Force delete actually removes files
// =============================================================================

#[test]
fn test_force_delete_removes_files() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with files
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/delete_me.txt"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Verify file exists before deletion
    assert!(source.join("user1/delete_me.txt").exists());

    // Run with --force to skip confirmation
    let (stdout, _stderr, success) = run_rm(&["-i", index.to_str().unwrap(), "--force"]);

    assert!(success);
    assert!(stdout.contains("Deleted: 1"));

    // Verify file is deleted
    assert!(!source.join("user1/delete_me.txt").exists());
}

// =============================================================================
// Test: Pattern filter works correctly
// =============================================================================

#[test]
fn test_pattern_filter_deletes_matching_files() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with mixed files
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/keep.txt"), 100).unwrap();
    create_test_file(&source.join("user1/delete.log"), 100).unwrap();
    create_test_file(&source.join("user1/also_delete.log"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Delete only .log files
    let (stdout, _stderr, success) = run_rm(&[
        "-i",
        index.to_str().unwrap(),
        "--pattern",
        "*.log",
        "--force",
    ]);

    assert!(success);
    assert!(stdout.contains("Deleted: 2"));

    // Verify correct files remain
    assert!(source.join("user1/keep.txt").exists());
    assert!(!source.join("user1/delete.log").exists());
    assert!(!source.join("user1/also_delete.log").exists());
}

// =============================================================================
// Test: --regex restores regular-expression matching
// =============================================================================

#[test]
fn test_regex_flag_restores_regex_matching() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Same mixed fixture as the glob pattern test above.
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/keep.txt"), 100).unwrap();
    create_test_file(&source.join("user1/delete.log"), 100).unwrap();
    create_test_file(&source.join("user1/also_delete.log"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // The old regex spelling keeps working behind the opt-in switch.
    let (stdout, _stderr, success) = run_rm(&[
        "-i",
        index.to_str().unwrap(),
        "--pattern",
        "\\.log$",
        "--regex",
        "--force",
    ]);

    assert!(success);
    assert!(stdout.contains("Deleted: 2"));

    // Verify correct files remain
    assert!(source.join("user1/keep.txt").exists());
    assert!(!source.join("user1/delete.log").exists());
    assert!(!source.join("user1/also_delete.log").exists());
}

// =============================================================================
// Test: An invalid glob deletes nothing
// =============================================================================

#[test]
fn test_invalid_glob_deletes_nothing() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/keep.log"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Rejection happens before any deletion set is selected, so --force is safe here.
    let (_stdout, stderr, success) =
        run_rm(&["-i", index.to_str().unwrap(), "--pattern", "[", "--force"]);

    assert!(!success);
    assert!(stderr.contains("Unterminated character class"));
    assert!(source.join("user1/keep.log").exists());
}

// =============================================================================
// Test: Size filters work correctly
// =============================================================================

#[test]
fn test_min_size_filter() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with files of different sizes
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/small.txt"), 100).unwrap();
    create_test_file(&source.join("user1/large.txt"), 10000).unwrap();

    // Build index
    build_index(&source, &index);

    // Delete only files >= 1K
    let (stdout, _stderr, success) =
        run_rm(&["-i", index.to_str().unwrap(), "--min-size", "1K", "--force"]);

    assert!(success);
    assert!(stdout.contains("Deleted: 1"));

    // Verify small file remains, large file deleted
    assert!(source.join("user1/small.txt").exists());
    assert!(!source.join("user1/large.txt").exists());
}

#[test]
fn test_max_size_filter() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with files of different sizes
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/small.txt"), 100).unwrap();
    create_test_file(&source.join("user1/large.txt"), 10000).unwrap();

    // Build index
    build_index(&source, &index);

    // Delete only files <= 1K
    let (stdout, _stderr, success) =
        run_rm(&["-i", index.to_str().unwrap(), "--max-size", "1K", "--force"]);

    assert!(success);
    assert!(stdout.contains("Deleted: 1"));

    // Verify large file remains, small file deleted
    assert!(!source.join("user1/small.txt").exists());
    assert!(source.join("user1/large.txt").exists());
}

// =============================================================================
// Test: Older-than filter works correctly
// =============================================================================

#[test]
fn test_older_than_filter() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with files
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/recent.txt"), 100).unwrap();
    create_test_file(&source.join("user1/old.txt"), 100).unwrap();

    // Set old.txt to have been accessed 90 days ago
    set_atime_days_ago(&source.join("user1/old.txt"), 90).unwrap();
    // recent.txt keeps current atime

    // Build index
    build_index(&source, &index);

    // Delete only files older than 30 days
    let (stdout, _stderr, success) = run_rm(&[
        "-i",
        index.to_str().unwrap(),
        "--older-than",
        "30",
        "--force",
    ]);

    assert!(success);
    assert!(stdout.contains("Deleted: 1"));

    // Verify recent file remains, old file deleted
    assert!(source.join("user1/recent.txt").exists());
    assert!(!source.join("user1/old.txt").exists());
}

// =============================================================================
// Test: Partition filter works correctly
// =============================================================================

#[test]
fn test_partition_filter() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create multiple partitions
    fs::create_dir_all(source.join("alice")).unwrap();
    fs::create_dir_all(source.join("bob")).unwrap();
    create_test_file(&source.join("alice/file.txt"), 100).unwrap();
    create_test_file(&source.join("bob/file.txt"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Delete only alice's files
    let (stdout, _stderr, success) = run_rm(&[
        "-i",
        index.to_str().unwrap(),
        "--partition",
        "alice",
        "--force",
    ]);

    assert!(success);
    assert!(stdout.contains("Deleted: 1"));

    // Verify bob's file remains, alice's file deleted
    assert!(!source.join("alice/file.txt").exists());
    assert!(source.join("bob/file.txt").exists());
}

// =============================================================================
// Test: Limit option works correctly
// =============================================================================

#[test]
fn test_limit_option() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with multiple files
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/file1.txt"), 100).unwrap();
    create_test_file(&source.join("user1/file2.txt"), 100).unwrap();
    create_test_file(&source.join("user1/file3.txt"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Delete only 1 file
    let (stdout, _stderr, success) =
        run_rm(&["-i", index.to_str().unwrap(), "--limit", "1", "--force"]);

    assert!(success);
    assert!(stdout.contains("Deleted: 1"));

    // Count remaining files (should be 2)
    let remaining: Vec<_> = fs::read_dir(source.join("user1"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(remaining.len(), 2);
}

// =============================================================================
// Test: --limit selects a deterministic, stable set (dry-run == real run)
// =============================================================================

#[test]
fn test_limit_deterministic_selection() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Five files with an unambiguous lexicographic order.
    fs::create_dir_all(source.join("user1")).unwrap();
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        create_test_file(&source.join("user1").join(name), 100).unwrap();
    }

    build_index(&source, &index);

    // Two dry-runs with --limit 3 must produce byte-identical output (R1).
    let (out1, _e1, ok1) = run_rm(&["-i", index.to_str().unwrap(), "--limit", "3", "--dry-run"]);
    let (out2, _e2, ok2) = run_rm(&["-i", index.to_str().unwrap(), "--limit", "3", "--dry-run"]);
    assert!(ok1 && ok2);
    assert_eq!(out1, out2, "two --dry-run --limit runs diverged");

    // The three lexicographically-smallest paths are selected (R3).
    assert!(out1.contains("a.txt"));
    assert!(out1.contains("b.txt"));
    assert!(out1.contains("c.txt"));
    assert!(!out1.contains("d.txt"));
    assert!(!out1.contains("e.txt"));
    assert!(out1.contains("3 file(s) would be deleted"));

    // The real run deletes exactly what the dry-run previewed (R2).
    let (out3, _e3, ok3) = run_rm(&["-i", index.to_str().unwrap(), "--limit", "3", "--force"]);
    assert!(ok3);
    assert!(out3.contains("Deleted: 3"));

    assert!(!source.join("user1/a.txt").exists());
    assert!(!source.join("user1/b.txt").exists());
    assert!(!source.join("user1/c.txt").exists());
    assert!(source.join("user1/d.txt").exists());
    assert!(source.join("user1/e.txt").exists());
}

// =============================================================================
// Test: Safe mode protects recently accessed files (CRITICAL TEST)
// =============================================================================

#[test]
fn test_safe_mode_protects_recently_accessed_files() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with an old file
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/important.txt"), 100).unwrap();

    // Set the file to be accessed 90 days ago
    set_atime_days_ago(&source.join("user1/important.txt"), 90).unwrap();

    // Build the index (captures the old atime)
    build_index(&source, &index);

    // Now simulate the user accessing the file AFTER the index was built
    // This updates the atime to now
    let _content = fs::read(source.join("user1/important.txt")).unwrap();

    // Attempt to delete with --safe mode
    // Without --safe, the file would be deleted because the INDEX shows it as old
    // With --safe, the file should be PROTECTED because its CURRENT atime is recent
    let (stdout, _stderr, success) = run_rm(&[
        "-i",
        index.to_str().unwrap(),
        "--older-than",
        "30",
        "--safe",
        "--force",
        "--verbose",
    ]);

    assert!(success);

    // The file should be skipped due to safe mode
    assert!(stdout.contains("SKIP (accessed since index)"));
    assert!(stdout.contains("Skipped (safe mode): 1"));
    assert!(stdout.contains("Deleted: 0"));

    // CRITICAL: The file must still exist
    assert!(source.join("user1/important.txt").exists());
}

// =============================================================================
// Test: Safe mode with max-size protects files that grew
// =============================================================================

#[test]
fn test_safe_mode_protects_files_that_grew() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with a small file
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/growing.txt"), 100).unwrap();

    // Build the index (captures the small size)
    build_index(&source, &index);

    // Now the file grows (user added data)
    {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(source.join("user1/growing.txt"))
            .unwrap();
        file.write_all(&vec![b'y'; 2000]).unwrap();
    }

    // Attempt to delete files <= 1K with --safe mode
    // Without --safe, it would try to delete (index shows 100 bytes)
    // With --safe, it should skip because current size is > 1K
    let (stdout, _stderr, success) = run_rm(&[
        "-i",
        index.to_str().unwrap(),
        "--max-size",
        "1K",
        "--safe",
        "--force",
        "--verbose",
    ]);

    assert!(success);

    // The file should be skipped due to safe mode
    assert!(stdout.contains("SKIP (size changed)"));
    assert!(stdout.contains("Skipped (safe mode): 1"));
    assert!(stdout.contains("Deleted: 0"));

    // CRITICAL: The file must still exist
    assert!(source.join("user1/growing.txt").exists());
}

// =============================================================================
// Test: Missing files are handled gracefully
// =============================================================================

#[test]
fn test_handles_missing_files_gracefully() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with files
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/exists.txt"), 100).unwrap();
    create_test_file(&source.join("user1/will_be_gone.txt"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Delete one file manually (simulating external deletion)
    fs::remove_file(source.join("user1/will_be_gone.txt")).unwrap();

    // Run xdu-rm
    let (stdout, _stderr, success) =
        run_rm(&["-i", index.to_str().unwrap(), "--force", "--verbose"]);

    assert!(success);
    assert!(stdout.contains("Missing: 1"));
    assert!(stdout.contains("Deleted: 1"));
}

// =============================================================================
// Test: Verbose output shows file-by-file status
// =============================================================================

#[test]
fn test_verbose_output() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with files
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/file1.txt"), 100).unwrap();
    create_test_file(&source.join("user1/file2.txt"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Run with verbose
    let (stdout, _stderr, success) =
        run_rm(&["-i", index.to_str().unwrap(), "--force", "--verbose"]);

    assert!(success);
    assert!(stdout.contains("DELETE:"));
    assert!(stdout.contains("file1.txt"));
    assert!(stdout.contains("file2.txt"));
}

// =============================================================================
// Test: No matching files produces appropriate message
// =============================================================================

#[test]
fn test_no_matching_files() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with files
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/file.txt"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Search for non-existent pattern
    let (stdout, _stderr, success) = run_rm(&[
        "-i",
        index.to_str().unwrap(),
        "--pattern",
        "*.nonexistent",
        "--force",
    ]);

    assert!(success);
    assert!(stdout.contains("No matching files found"));
}

// =============================================================================
// Test: Combined filters work together
// =============================================================================

#[test]
fn test_combined_filters() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with various files
    fs::create_dir_all(source.join("user1")).unwrap();

    // File that matches all criteria (old, .log, large enough)
    create_test_file(&source.join("user1/old_large.log"), 2000).unwrap();
    set_atime_days_ago(&source.join("user1/old_large.log"), 90).unwrap();

    // File that's old but wrong extension
    create_test_file(&source.join("user1/old.txt"), 2000).unwrap();
    set_atime_days_ago(&source.join("user1/old.txt"), 90).unwrap();

    // File that's .log but recent
    create_test_file(&source.join("user1/recent.log"), 2000).unwrap();

    // File that's old and .log but too small
    create_test_file(&source.join("user1/old_small.log"), 100).unwrap();
    set_atime_days_ago(&source.join("user1/old_small.log"), 90).unwrap();

    // Build index
    build_index(&source, &index);

    // Delete only files that are: .log AND older than 30 days AND >= 1K
    let (stdout, _stderr, success) = run_rm(&[
        "-i",
        index.to_str().unwrap(),
        "--pattern",
        "*.log",
        "--older-than",
        "30",
        "--min-size",
        "1K",
        "--force",
    ]);

    assert!(success);
    assert!(stdout.contains("Deleted: 1"));

    // Only old_large.log should be deleted
    assert!(!source.join("user1/old_large.log").exists());
    assert!(source.join("user1/old.txt").exists());
    assert!(source.join("user1/recent.log").exists());
    assert!(source.join("user1/old_small.log").exists());
}

// =============================================================================
// Test: Safe mode without relevant filter has no effect
// =============================================================================

#[test]
fn test_safe_mode_without_relevant_filter_still_deletes() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("source");
    let index = temp_dir.path().join("index");

    // Create partition with a file
    fs::create_dir_all(source.join("user1")).unwrap();
    create_test_file(&source.join("user1/file.txt"), 100).unwrap();

    // Build index
    build_index(&source, &index);

    // Run with --safe but no --older-than or --max-size
    // Safe mode has nothing to check, so deletion proceeds
    let (stdout, _stderr, success) = run_rm(&["-i", index.to_str().unwrap(), "--safe", "--force"]);

    assert!(success);
    assert!(stdout.contains("Deleted: 1"));
    assert!(!source.join("user1/file.txt").exists());
}

// =============================================================================
// Test: An S3 index locator is refused before anything else runs
// =============================================================================

/// `xdu-rm` refuses an `s3://` locator from `-i` or `XDU_INDEX` with one line and exit 1. A
/// garbage `httpfs` stands in `XDU_EXTENSION_DIR`, so an extension load would have printed its
/// own failure; an index lookup or a query would have printed theirs, and a selection would
/// have printed to stdout. The one refusal line, and nothing else, is the proof none ran.
#[test]
fn test_s3_locator_is_refused_before_anything_runs() {
    let extensions = TempDir::new().unwrap();
    fs::write(
        extensions.path().join("httpfs.duckdb_extension"),
        b"not a DuckDB extension",
    )
    .unwrap();
    let hermetic = common::s3::Hermetic::new();
    for via_env in [false, true] {
        let mut cmd = hermetic.command("xdu-rm");
        cmd.env("XDU_EXTENSION_DIR", extensions.path())
            .args(["--force", "-j", "2"]);
        if via_env {
            cmd.env("XDU_INDEX", "s3://bucket/idx");
        } else {
            cmd.args(["-i", "s3://bucket/idx"]);
        }
        let output = cmd.output().expect("spawn xdu-rm");
        assert_eq!(output.status.code(), Some(1), "via_env={via_env}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "xdu-rm: S3 indices are not yet supported by xdu-rm\n",
            "via_env={via_env}"
        );
        assert!(output.stdout.is_empty(), "via_env={via_env}");
    }
    hermetic.assert_left_empty();
}

// =============================================================================
// A key=value directory name reads like any other
// =============================================================================
//
// DuckDB's Hive detection turns a `key=value` directory name that every globbed path carries
// into a column `key` that shadows the row's own, and xdu-rm deletes the rows a shadowed column
// selects. Each case compares an index whose partition or index directory has such a name
// against a bare twin built from the same files. The fixtures stay in this file rather than
// `tests/common/mod.rs`, which these tests must not change: they have to build on a release
// base that lacks that file's later helpers.
//
// Every run against an index under `path=0/` is a dry run. Where the defect is present, the
// shadowed `path` column holds the directory's value, and a real run would unlink that
// relative name in the test's working directory.

/// The one partition of the partition layout, with apparent sizes.
const PARTITION_FILES: &[(&str, usize)] = &[
    ("big.bin", 5_000_000),
    ("small.txt", 1),
    ("sub/mid.txt", 300),
];

/// The tree of the index-directory layout: two partitions and a loose file (`__root__`).
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
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    build_index(tree, index);
}

/// A preview's lines, sorted and without the blank separator, since two crawls of one tree
/// need not store its rows in the same order.
fn preview(out: &str) -> Vec<String> {
    let mut lines: Vec<String> = out
        .lines()
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect();
    lines.sort();
    lines
}

/// The lines `preview` should return for a dry run selecting exactly `paths`.
fn previewing(paths: &[String]) -> Vec<String> {
    let mut lines = paths.to_vec();
    lines.push(format!("{} file(s) would be deleted.", paths.len()));
    lines.sort();
    lines
}

/// A partition named `size=0` reads as a name: the set xdu-rm previews and the set it deletes
/// are those a bare name selects, and a size filter meant to spare a file spares it.
#[test]
fn rm_reads_a_key_value_partition_as_a_bare_name() {
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    write_tree(&source.join("size-0"), PARTITION_FILES, now_secs());
    // The twin is indexed from the same files while the partition still has a bare name; the
    // rename keeps every inode, so the two indexes differ in that one name.
    let twin = work.path().join("twin");
    build_index(&source, &twin);
    fs::rename(source.join("size-0"), source.join("size=0")).unwrap();
    let index = work.path().join("index");
    build_index(&source, &index);
    let root = source.canonicalize().unwrap().to_str().unwrap().to_string();
    let (index, twin) = (index.to_str().unwrap(), twin.to_str().unwrap());
    let small = [
        format!("{root}/size=0/small.txt"),
        format!("{root}/size=0/sub/mid.txt"),
    ];

    let (out, err, ok) = run_rm(&["-i", index, "-u", "size=0", "--max-size", "1K", "-n"]);
    assert!(
        ok && preview(&out) == previewing(&small),
        "key=value partition: -u size=0 --max-size 1K -n previewed {out:?}, not {small:?} \
         (stderr: {err})"
    );

    let (out, err, ok) = run_rm(&["-i", index, "--min-size", "1M", "-n"]);
    assert!(ok, "{err}");
    assert_eq!(
        preview(&out),
        previewing(&[format!("{root}/size=0/big.bin")]),
        "--min-size 1M -n"
    );

    let (bare_dir, kv_dir) = (format!("{root}/size-0/"), format!("{root}/size=0/"));
    let flag_sets: &[&[&str]] = &[
        &["-p", "*.txt"],
        &["--limit", "1"],
        &["--max-size", "1K", "--safe"],
    ];
    let mut compared = 0;
    for scope in [None, Some(("size=0", "size-0"))] {
        for flags in flag_sets {
            let mut args = vec!["-i", index, "-n"];
            let mut twin_args = vec!["-i", twin, "-n"];
            if let Some((name, twin_name)) = scope {
                args.extend(["-u", name]);
                twin_args.extend(["-u", twin_name]);
            }
            args.extend_from_slice(flags);
            twin_args.extend_from_slice(flags);
            let (out, err, ok) = run_rm(&args);
            assert!(ok, "{args:?}: {err}");
            let (twin_out, err, ok) = run_rm(&twin_args);
            assert!(ok, "{twin_args:?}: {err}");
            assert!(
                twin_out.contains("would be deleted"),
                "{twin_args:?} previewed nothing: {twin_out:?}"
            );
            assert_eq!(
                preview(&out),
                preview(&twin_out.replace(&bare_dir, &kv_dir)),
                "{args:?} against the bare twin"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 2 * flag_sets.len());

    // The real run, with and without --safe, each on a fresh tree and index.
    for safe in [false, true] {
        let work = TempDir::new().unwrap();
        let source = work.path().join("source");
        let partition = source.join("size=0");
        write_tree(&partition, PARTITION_FILES, now_secs());
        let index = work.path().join("index");
        build_index(&source, &index);
        let mut args = vec![
            "-i",
            index.to_str().unwrap(),
            "-u",
            "size=0",
            "--max-size",
            "1K",
            "--force",
        ];
        if safe {
            args.push("--safe");
        }
        let (_out, err, ok) = run_rm(&args);
        assert!(ok, "{args:?}: {err}");
        assert!(
            partition.join("big.bin").exists(),
            "{args:?} deleted big.bin"
        );
        assert!(
            !partition.join("small.txt").exists(),
            "{args:?} kept small.txt"
        );
        assert!(
            !partition.join("sub/mid.txt").exists(),
            "{args:?} kept sub/mid.txt"
        );
    }
}

/// An index stored under a `key=value` directory reads as one stored under a bare name: an age
/// filter meant to spare fresh files spares them, and every filter previews what the bare index
/// previews.
#[test]
fn rm_reads_an_index_under_a_key_value_directory_as_a_bare_one() {
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

    let atime = work.path().join("atime=0/index");
    let atime = atime.to_str().unwrap();
    let (out, err, ok) = run_rm(&["-i", atime, "--older-than", "3650", "-n"]);
    assert!(
        ok && out == "No matching files found.\n",
        "key=value index directory: under atime=0, --older-than 3650 -n previewed files \
         accessed minutes ago: {out:?} (stderr: {err})"
    );

    let (out, err, ok) = run_rm(&["-i", atime, "--older-than", "3650", "--force"]);
    assert!(ok, "{err}");
    assert_eq!(
        out, "No matching files found.\n",
        "the real run under atime=0"
    );
    for (rel, _) in INDEX_DIR_FILES {
        assert!(
            tree.join(rel).exists(),
            "the real run under atime=0 deleted {rel}"
        );
    }

    let flag_sets: &[&[&str]] = &[
        &[],
        &["-p", "*.txt"],
        &["--older-than", "3650"],
        &["--newer-than", "1"],
        &["--min-size", "1"],
        &["--max-size", "1K"],
        &["-u", "alpha"],
        &["--limit", "1"],
    ];
    let mut compared = 0;
    for flags in flag_sets {
        let mut args = vec!["-i", bare.to_str().unwrap(), "-n"];
        args.extend_from_slice(flags);
        let (want, err, ok) = run_rm(&args);
        assert!(ok, "{args:?}: {err}");
        // The fixture is minutes old, so only the 3650-day filter selects nothing.
        let empty = flags.first() == Some(&"--older-than");
        assert_eq!(
            want == "No matching files found.\n",
            empty,
            "{args:?} on the bare index: {want:?}"
        );
        for (key, index) in KEYS.iter().zip(&keyed) {
            let mut args = vec!["-i", index.to_str().unwrap(), "-n"];
            args.extend_from_slice(flags);
            let (out, err, ok) = run_rm(&args);
            assert!(ok, "under {key}=0, {flags:?}: {err}");
            assert_eq!(
                preview(&out),
                preview(&want),
                "under {key}=0, {flags:?} -n against the bare index"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, flag_sets.len() * KEYS.len());
    for (rel, _) in INDEX_DIR_FILES {
        assert!(tree.join(rel).exists(), "a dry run deleted {rel}");
    }
}
