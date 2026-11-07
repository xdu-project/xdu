//! Integration tests for `xdu-cp`, driving the real binary against throwaway indexes.
//!
//! Index rows hold the canonical crawl root, so every fixture works in canonical paths and
//! passes `--root` as the canonical source tree (on macOS a `TempDir` under `/var` is indexed
//! as `/private/var`). Indexes use `--apparent-size` so size filters are exact.

mod common;

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use common::{
    append_bytes, binary_path, build_index, build_index_block_size, create_test_file,
    list_files_recursive, run_cp, run_find, run_with_stdin, set_atime_days_ago, set_mtime_days_ago,
};

struct Fixture {
    _dir: TempDir,
    source: PathBuf,
    index: PathBuf,
    into: PathBuf,
}

impl Fixture {
    /// A source tree of `files` (relative path, size), indexed, and an empty `--into`.
    fn new(files: &[(&str, usize)]) -> Fixture {
        Fixture::with_index_name(files, "index")
    }

    fn with_index_name(files: &[(&str, usize)], index_name: &str) -> Fixture {
        let dir = TempDir::new().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        for (rel, size) in files {
            create_test_file(&source.join(rel), *size).unwrap();
        }
        let into = base.join("into");
        fs::create_dir_all(&into).unwrap();
        Fixture {
            _dir: dir,
            source,
            index: base.join(index_name),
            into,
        }
    }

    fn build(self) -> Fixture {
        build_index(&self.source, &self.index);
        self
    }

    fn s(path: &Path) -> &str {
        path.to_str().unwrap()
    }

    /// `-i INDEX --into INTO --root SOURCE`, then `extra`.
    fn args<'a>(&'a self, extra: &[&'a str]) -> Vec<&'a str> {
        let mut args = vec![
            "-i",
            Fixture::s(&self.index),
            "--into",
            Fixture::s(&self.into),
            "--root",
            Fixture::s(&self.source),
        ];
        args.extend_from_slice(extra);
        args
    }

    fn src(&self, rel: &str) -> PathBuf {
        self.source.join(rel)
    }

    fn dest(&self, rel: &str) -> PathBuf {
        self.into.join(rel)
    }

    /// Every file under `--into`, as paths relative to it.
    fn copied(&self) -> Vec<String> {
        list_files_recursive(&self.into)
            .iter()
            .map(|p| {
                p.strip_prefix(&self.into)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    /// Nothing at all under `--into`: no file, no directory.
    fn nothing_under_into(&self) -> bool {
        fs::read_dir(&self.into).unwrap().next().is_none()
    }
}

/// The sources a dry-run lists, in its order.
fn dry_run_sources(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| l.split_once(" -> ").map(|(src, _)| src.to_string()))
        .collect()
}

fn find_paths_with(index: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["-i", index.to_str().unwrap()];
    args.extend_from_slice(extra);
    let (out, err, ok) = run_find(&args);
    assert!(ok, "xdu-find {extra:?} failed: {err}");
    let mut paths: Vec<String> = out.lines().map(str::to_string).collect();
    paths.sort();
    paths
}

fn mkfifo(path: &Path) {
    let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: a valid C string for the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
}

#[test]
fn cp_copies_exactly_the_selected_set_under_root() {
    let fx = Fixture::new(&[
        ("alice/logs/app.log", 2048),
        ("alice/data.bin", 4096),
        ("bob/notes.txt", 1024),
    ])
    .build();
    let (out, err, ok) = run_cp(&fx.args(&["-u", "alice", "--force"]));
    assert!(ok, "{err}");
    assert!(out.contains("Copied: 2"), "{out}");
    assert_eq!(fx.copied(), ["alice/data.bin", "alice/logs/app.log"]);
    for rel in ["alice/logs/app.log", "alice/data.bin"] {
        assert_eq!(
            fs::read(fx.dest(rel)).unwrap(),
            fs::read(fx.src(rel)).unwrap()
        );
        assert!(fx.src(rel).is_file(), "{rel}: the source must stay");
    }
    assert!(fx.src("bob/notes.txt").is_file());

    // Without --root the whole absolute path is reproduced under --into; XDU_INDEX and
    // XDU_JOBS stand in for -i and -j.
    let whole = TempDir::new().unwrap();
    let whole_into = whole.path().canonicalize().unwrap();
    let output = Command::new(binary_path("xdu-cp"))
        .env("XDU_INDEX", &fx.index)
        .env("XDU_JOBS", "2")
        .args([
            "-u",
            "bob",
            "--force",
            "--into",
            whole_into.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mirrored = whole_into.join(fx.src("bob/notes.txt").strip_prefix("/").unwrap());
    assert_eq!(fs::read(&mirrored).unwrap().len(), 1024);
    assert_eq!(list_files_recursive(&whole_into), vec![mirrored]);
}

#[test]
fn cp_dry_run_writes_nothing() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("bob/deep/b.txt", 20)]).build();
    let (out, err, ok) = run_cp(&fx.args(&["--dry-run"]));
    assert!(ok, "{err}");
    let expected = [
        format!(
            "{} -> {}",
            fx.src("alice/a.txt").display(),
            fx.dest("alice/a.txt").display()
        ),
        format!(
            "{} -> {}",
            fx.src("bob/deep/b.txt").display(),
            fx.dest("bob/deep/b.txt").display()
        ),
    ];
    let listed: Vec<&str> = out.lines().filter(|l| l.contains(" -> ")).collect();
    assert_eq!(listed, expected);
    assert!(
        out.contains(&format!(
            "2 file(s) would be copied into {}",
            fx.into.display()
        )),
        "{out}"
    );
    assert!(
        fx.nothing_under_into(),
        "a dry run created something under --into"
    );
    assert!(fx.src("alice/a.txt").is_file() && fx.src("bob/deep/b.txt").is_file());
}

#[test]
fn cp_dry_run_and_the_real_run_select_identical_rows_under_limit() {
    let names = ["u/f", "u/c", "u/e", "u/a", "u/d", "u/b"];
    let files: Vec<(&str, usize)> = names.iter().map(|n| (*n, 8)).collect();
    let fx = Fixture::new(&files).build();
    let (out, err, ok) = run_cp(&fx.args(&["-l", "3", "-n"]));
    assert!(ok, "{err}");
    let previewed = dry_run_sources(&out);
    let expected: Vec<String> = ["u/a", "u/b", "u/c"]
        .iter()
        .map(|r| fx.src(r).display().to_string())
        .collect();
    assert_eq!(
        previewed, expected,
        "the limit takes the first paths in order"
    );

    let (out, err, ok) = run_cp(&fx.args(&["-l", "3", "--force"]));
    assert!(ok, "{err}");
    assert!(out.contains("Copied: 3"), "{out}");
    let copied: Vec<String> = fx
        .copied()
        .iter()
        .map(|r| fx.src(r).display().to_string())
        .collect();
    assert_eq!(copied, previewed);
}

/// The S3 refusal precedes everything: a garbage `httpfs` in `XDU_EXTENSION_DIR` would have
/// printed its own failure had any extension loaded, and a selection would have printed to
/// stdout. The one refusal line, and nothing written, is the proof none ran.
#[test]
fn cp_refuses_an_s3_index_before_anything() {
    let extensions = TempDir::new().unwrap();
    let garbage = extensions.path().join("httpfs.duckdb_extension");
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(garbage)
        .unwrap();
    std::io::Write::write_all(&mut f, b"not a DuckDB extension").unwrap();
    let into = TempDir::new().unwrap();
    let hermetic = common::s3::Hermetic::new();
    for via_env in [false, true] {
        let mut cmd = hermetic.command("xdu-cp");
        cmd.env("XDU_EXTENSION_DIR", extensions.path()).args([
            "--force",
            "-j",
            "2",
            "--into",
            into.path().to_str().unwrap(),
        ]);
        if via_env {
            cmd.env("XDU_INDEX", "s3://bucket/idx");
        } else {
            cmd.args(["-i", "s3://bucket/idx"]);
        }
        let output = cmd.output().expect("spawn xdu-cp");
        assert_eq!(output.status.code(), Some(1), "via_env={via_env}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "xdu-cp: S3 indices are not supported by xdu-cp\n",
            "via_env={via_env}"
        );
        assert!(output.stdout.is_empty(), "via_env={via_env}");
    }
    assert_eq!(fs::read_dir(into.path()).unwrap().count(), 0);
    hermetic.assert_left_empty();
}

#[test]
fn cp_refuses_a_missing_or_non_directory_into() {
    let fx = Fixture::new(&[("alice/a.txt", 10)]).build();
    let missing = fx.into.join("absent");
    let (out, err, ok) = run_cp(&[
        "-i",
        Fixture::s(&fx.index),
        "--into",
        Fixture::s(&missing),
        "--force",
    ]);
    assert!(!ok);
    assert!(err.contains("does not exist"), "{err}");
    assert!(out.is_empty(), "{out}");
    assert!(!missing.exists());

    let file = fx.into.join("plain");
    create_test_file(&file, 3).unwrap();
    let (out, err, ok) = run_cp(&[
        "-i",
        Fixture::s(&fx.index),
        "--into",
        Fixture::s(&file),
        "--force",
    ]);
    assert!(!ok);
    assert!(err.contains("is not a directory"), "{err}");
    assert!(out.is_empty(), "{out}");
    assert_eq!(fs::read(&file).unwrap().len(), 3);
    assert_eq!(fx.copied(), ["plain"]);
}

#[test]
fn cp_refuses_a_path_outside_root_naming_the_first() {
    let fx = Fixture::new(&[
        ("alice/a.txt", 10),
        ("bob/b1.txt", 10),
        ("bob/b2.txt", 10),
        ("carol/c.txt", 10),
    ])
    .build();
    let alice = fx.src("alice");
    for extra in [vec!["--force"], vec!["--dry-run"]] {
        let mut args = vec![
            "-i",
            Fixture::s(&fx.index),
            "--into",
            Fixture::s(&fx.into),
            "--root",
            Fixture::s(&alice),
        ];
        args.extend(extra.iter().copied());
        let (out, err, ok) = run_cp(&args);
        assert!(!ok, "{extra:?}");
        let first = fx.src("bob/b1.txt").display().to_string();
        assert!(
            err.contains(&format!("{first} is not under --root")),
            "{err}"
        );
        assert!(!err.contains("b2.txt") && !err.contains("carol"), "{err}");
        assert!(!out.contains(" -> "), "{out}");
        assert!(fx.nothing_under_into());
    }
}

#[test]
fn cp_refuses_a_destination_equal_to_its_source() {
    let fx = Fixture::new(&[("alice/a.txt", 10)]).build();
    let (out, err, ok) = run_cp(&[
        "-i",
        Fixture::s(&fx.index),
        "--into",
        Fixture::s(&fx.source),
        "--root",
        Fixture::s(&fx.source),
        "--force",
    ]);
    assert!(!ok);
    let path = fx.src("alice/a.txt").display().to_string();
    assert!(
        err.contains(&format!(
            "{path} maps to a destination equal to its own source path"
        )),
        "{err}"
    );
    assert!(out.is_empty(), "{out}");
    assert_eq!(fs::read(fx.src("alice/a.txt")).unwrap().len(), 10);
    assert_eq!(list_files_recursive(&fx.source).len(), 1);
}

/// A quote in the index path or the partition name is a byte like any other: the glob is a
/// bound value, so the right rows come back instead of a parser error or injected SQL.
#[test]
fn cp_selects_the_right_rows_with_a_quote_in_the_index_and_partition() {
    let fx = Fixture::with_index_name(
        &[
            ("al'ice/a.txt", 11),
            ("al'ice/sub/b.txt", 12),
            ("bob/c.txt", 13),
        ],
        "idx'q",
    )
    .build();
    let (out, err, ok) = run_cp(&fx.args(&["-u", "al'ice", "--force"]));
    assert!(ok, "{err}");
    assert!(out.contains("Copied: 2"), "{out}");
    assert_eq!(fx.copied(), ["al'ice/a.txt", "al'ice/sub/b.txt"]);

    let (out, err, ok) = run_cp(&fx.args(&["-p", "*'ice/a.txt", "-n"]));
    assert!(ok, "{err}");
    assert_eq!(
        dry_run_sources(&out),
        [fx.src("al'ice/a.txt").display().to_string()]
    );
}

/// A `key=value` directory name is a name like any other: a partition called `size=0` must
/// not stand in for its rows' sizes, or a size filter selects files the index does not describe.
#[test]
fn cp_selects_the_true_rows_under_a_hive_style_partition() {
    let fx = Fixture::new(&[("size=0/big.dat", 5_000_000), ("alice/small.dat", 1)]).build();
    let big = fx.src("size=0/big.dat").display().to_string();
    let (out, err, ok) = run_cp(&fx.args(&["-u", "size=0", "--max-size", "1K", "-n"]));
    assert!(ok, "{err}");
    assert!(out.contains("No matching files found."), "{out}");
    let (out, err, ok) = run_cp(&fx.args(&["-u", "size=0", "--min-size", "1M", "-n"]));
    assert!(ok, "{err}");
    assert_eq!(dry_run_sources(&out), [big], "{out}");
    let (out, err, ok) = run_cp(&fx.args(&["-u", "size=0", "--max-size", "1K", "--force"]));
    assert!(ok, "{err}");
    assert!(out.contains("No matching files found."), "{out}");
    assert!(fx.nothing_under_into());
}

/// An index stored under a `mtime=0` directory must not give every row an mtime of 0. The
/// files were written moments ago, so none is older than ten years by mtime.
#[test]
fn cp_selects_the_true_rows_from_an_index_under_a_hive_style_directory() {
    let fx = Fixture::with_index_name(&[("alice/a.dat", 10), ("bob/b.dat", 20)], "mtime=0/index");
    fs::create_dir_all(fx.index.parent().unwrap()).unwrap();
    let fx = fx.build();
    let (out, err, ok) = run_cp(&fx.args(&["--mtime-older-than", "3650", "-n"]));
    assert!(ok, "{err}");
    assert!(out.contains("No matching files found."), "{out}");
    let (out, err, ok) = run_cp(&fx.args(&["--mtime-newer-than", "1", "--min-size", "15", "-n"]));
    assert!(ok, "{err}");
    assert_eq!(
        dry_run_sources(&out),
        [fx.src("bob/b.dat").display().to_string()],
        "{out}"
    );
}

/// A bound glob still globs: an index at `idx[ab]` would read a sibling `idxa`. The engine
/// refuses the path rather than act on another index's files.
#[test]
fn cp_refuses_a_glob_metacharacter_in_the_index_path() {
    let fx = Fixture::with_index_name(&[("alice/a.txt", 10)], "idx[ab]").build();
    let sibling = fx.index.with_file_name("idxa");
    let other = TempDir::new().unwrap();
    create_test_file(&other.path().join("mallory/m.txt"), 5).unwrap();
    build_index(other.path(), &sibling);

    for extra in [vec!["--force"], vec!["--dry-run"]] {
        let mut args = vec!["-i", Fixture::s(&fx.index), "--into", Fixture::s(&fx.into)];
        args.extend(extra.iter().copied());
        let (out, err, ok) = run_cp(&args);
        assert!(!ok, "{extra:?}");
        assert!(err.contains("glob character '['"), "{err}");
        assert!(out.is_empty(), "{out}");
        assert!(fx.nothing_under_into());
    }
}

#[test]
fn cp_refuses_a_partition_that_does_not_exist() {
    let fx = Fixture::new(&[("alice/a.txt", 10)]).build();
    let (out, err, ok) = run_cp(&fx.args(&["-u", "nobody", "--force"]));
    assert!(!ok);
    assert!(err.contains("no partition nobody in "), "{err}");
    assert!(out.is_empty(), "{out}");
    assert!(fx.nothing_under_into());

    let (_, err, ok) = run_cp(&fx.args(&["-u", "../alice", "--force"]));
    assert!(!ok);
    assert!(err.contains("does not name one directory"), "{err}");
    assert!(fx.nothing_under_into());
}

#[test]
fn cp_counts_an_existing_destination_and_exits_non_zero() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("alice/b.txt", 20), ("bob/c.txt", 30)]).build();
    fs::create_dir_all(fx.dest("alice")).unwrap();
    create_test_file(&fx.dest("alice/b.txt"), 3).unwrap();
    let (out, err, ok) = run_cp(&fx.args(&["--force", "-v"]));
    assert!(!ok, "an existing destination must exit non-zero");
    assert!(out.contains("Copied: 2"), "{out}");
    assert!(out.contains("Existing at destination: 1"), "{out}");
    assert!(
        out.contains(&format!("existing: {}", fx.dest("alice/b.txt").display())),
        "{out}"
    );
    assert!(
        !err.contains("b.txt"),
        "an existing file is not a failure: {err}"
    );
    assert_eq!(
        fs::read(fx.dest("alice/b.txt")).unwrap().len(),
        3,
        "replaced"
    );
    assert_eq!(fs::read(fx.dest("alice/a.txt")).unwrap().len(), 10);
    assert_eq!(fs::read(fx.dest("bob/c.txt")).unwrap().len(), 30);
}

#[test]
fn cp_declined_prompt_writes_nothing() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("bob/b.txt", 10)]).build();
    for input in [Some("n\n"), Some("\n"), Some("yep\n"), None] {
        let (out, err, ok) = run_with_stdin("xdu-cp", &fx.args(&[]), input);
        assert!(ok, "{input:?}: {err}");
        assert!(
            err.contains(&format!(
                "Copy 2 file(s) into {}? [y/N] ",
                fx.into.display()
            )),
            "{input:?}: {err}"
        );
        assert!(err.contains("Aborted."), "{input:?}: {err}");
        assert!(!out.contains("Copied"), "{input:?}: {out}");
        assert!(fx.nothing_under_into(), "{input:?}: something was written");
    }
}

#[test]
fn cp_accepted_prompt_copies() {
    for answer in ["y\n", " YES \n"] {
        let fx = Fixture::new(&[("alice/a.txt", 10), ("bob/b.txt", 10)]).build();
        let (out, err, ok) = run_with_stdin("xdu-cp", &fx.args(&[]), Some(answer));
        assert!(ok, "{answer:?}: {err}");
        assert!(
            err.contains("Copy 2 file(s) into"),
            "the prompt is on stderr: {err}"
        );
        assert!(!out.contains("[y/N]"), "{out}");
        assert!(out.contains("Copied: 2"), "{answer:?}: {out}");
        assert_eq!(fx.copied(), ["alice/a.txt", "bob/b.txt"]);
    }
}

#[test]
fn cp_empty_match_exits_zero() {
    let fx = Fixture::new(&[("alice/a.txt", 10)]).build();
    let (out, err, ok) = run_cp(&fx.args(&["-p", "*.nomatch", "--force"]));
    assert!(ok, "{err}");
    assert_eq!(out.trim(), "No matching files found.");
    assert!(fx.nothing_under_into());
}

#[test]
fn cp_skips_a_symlink_at_the_source_path() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("alice/link.txt", 10)]).build();
    let outside = fx.source.with_file_name("outside.txt");
    create_test_file(&outside, 77).unwrap();
    fs::remove_file(fx.src("alice/link.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, fx.src("alice/link.txt")).unwrap();

    let (out, err, ok) = run_cp(&fx.args(&["--force", "-v"]));
    assert!(ok, "{err}");
    assert!(out.contains("Copied: 1"), "{out}");
    assert!(out.contains("Skipped: 1"), "{out}");
    assert!(
        out.contains(&format!(
            "skipped (not a regular file): {}",
            fx.src("alice/link.txt").display()
        )),
        "{out}"
    );
    assert_eq!(fx.copied(), ["alice/a.txt"]);
    assert!(fs::symlink_metadata(fx.dest("alice/link.txt")).is_err());
}

#[test]
fn cp_skips_a_fifo_and_a_directory_at_the_source_path() {
    let fx = Fixture::new(&[("u/a.txt", 10), ("u/fifo", 10), ("u/dir", 10)]).build();
    fs::remove_file(fx.src("u/fifo")).unwrap();
    mkfifo(&fx.src("u/fifo"));
    fs::remove_file(fx.src("u/dir")).unwrap();
    fs::create_dir(fx.src("u/dir")).unwrap();
    create_test_file(&fx.src("u/dir/inner.txt"), 4).unwrap();

    let (out, err, ok) = run_cp(&fx.args(&["--force"]));
    assert!(ok, "{err}");
    assert!(out.contains("Copied: 1"), "{out}");
    assert!(out.contains("Skipped: 2"), "{out}");
    assert_eq!(fx.copied(), ["u/a.txt"]);
    assert!(fs::symlink_metadata(fx.dest("u/fifo")).is_err());
    assert!(fs::symlink_metadata(fx.dest("u/dir")).is_err());
}

/// Each `xdu-find` filter selects the same set through `xdu-cp` as through `xdu-find`, alone
/// and all together.
#[test]
fn cp_accepts_every_xdu_find_filter() {
    let fx = Fixture::new(&[
        ("alice/old.log", 2048),
        ("alice/new.log", 512),
        ("alice/big.bin", 8192),
        ("bob/old.txt", 100),
        ("bob/new.txt", 4096),
    ]);
    for rel in ["alice/old.log", "alice/big.bin", "bob/old.txt"] {
        set_atime_days_ago(&fx.src(rel), 100).unwrap();
        set_mtime_days_ago(&fx.src(rel), 100).unwrap();
    }
    for (rel, mode) in [
        ("alice/old.log", 0o644),
        ("alice/new.log", 0o600),
        ("alice/big.bin", 0o640),
        ("bob/old.txt", 0o644),
        ("bob/new.txt", 0o604),
    ] {
        fs::set_permissions(fx.src(rel), fs::Permissions::from_mode(mode)).unwrap();
    }
    let fx = fx.build();
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() }.to_string();
    let gid = std::os::unix::fs::MetadataExt::gid(&fs::metadata(fx.src("alice/old.log")).unwrap())
        .to_string();

    let sets: Vec<Vec<&str>> = vec![
        vec!["-p", "*.log"],
        vec!["--regex", "-p", "\\.txt$"],
        vec!["-u", "bob"],
        vec!["--min-size", "2K"],
        vec!["--max-size", "1K"],
        vec!["--older-than", "30"],
        vec!["--newer-than", "30"],
        vec!["--owner", &uid],
        vec!["--group", &gid],
        vec!["--mode", "644"],
        vec!["--mode", "/004"],
        vec!["--mode", "&640"],
        vec!["--mtime-older-than", "30"],
        vec!["--mtime-newer-than", "30"],
        vec![
            "-u",
            "alice",
            "-p",
            "*.log",
            "--min-size",
            "1K",
            "--max-size",
            "4K",
            "--older-than",
            "30",
            "--newer-than",
            "400",
            "--owner",
            &uid,
            "--group",
            &gid,
            "--mode",
            "644",
            "--mtime-older-than",
            "30",
            "--mtime-newer-than",
            "400",
        ],
    ];
    let everything = find_paths_with(&fx.index, &[]);
    assert_eq!(everything.len(), 5);
    for set in &sets {
        let expected = find_paths_with(&fx.index, set);
        assert!(
            !expected.is_empty(),
            "{set:?} selects nothing; the case is vacuous"
        );
        let mut args = set.clone();
        args.push("-n");
        let (out, err, ok) = run_cp(&fx.args(&args));
        assert!(ok, "{set:?}: {err}");
        let mut got = dry_run_sources(&out);
        got.sort();
        assert_eq!(got, expected, "{set:?}");
    }
    let all = sets.last().unwrap();
    assert_eq!(
        find_paths_with(&fx.index, all),
        [fx.src("alice/old.log").display().to_string()]
    );
}

#[test]
fn cp_reports_failures_on_stderr_and_exits_non_zero() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("bob/b.txt", 10)]).build();
    // A regular file where the replica needs the directory `alice`.
    create_test_file(&fx.dest("alice"), 1).unwrap();
    let (out, err, ok) = run_cp(&fx.args(&["--force"]));
    assert!(!ok, "a failed file must exit non-zero");
    let line = format!(
        "xdu-cp: {}: destination component alice is not a directory",
        fx.src("alice/a.txt").display()
    );
    assert!(err.contains(&line), "{err}");
    assert!(
        !out.contains("not a directory"),
        "failures stay off stdout: {out}"
    );
    assert!(out.contains("Copied: 1"), "{out}");
    assert!(out.contains("Failed: 1"), "{out}");
    assert_eq!(fs::read(fx.dest("alice")).unwrap().len(), 1);
    assert_eq!(fs::read(fx.dest("bob/b.txt")).unwrap().len(), 10);
}

// ---- Metadata, --safe and the counted summary ----

fn meta(path: &Path) -> fs::Metadata {
    fs::symlink_metadata(path).unwrap()
}

/// Set a path's atime and mtime to the given (seconds, nanoseconds), not following a symlink.
#[allow(clippy::unnecessary_cast)]
fn set_times(path: &Path, atime: (i64, i64), mtime: (i64, i64)) {
    let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
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
    // SAFETY: a valid C string and a two-element timespec array for the call.
    let ret = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            c.as_ptr(),
            times.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    assert_eq!(ret, 0, "utimensat {}", path.display());
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn chgrp(path: &Path, gid: u32) -> std::io::Result<()> {
    std::os::unix::fs::lchown(path, None, Some(gid))
}

/// Every group the caller may give its own files: the effective group, then the
/// supplementary ones, without repeats.
fn my_groups() -> Vec<u32> {
    // SAFETY: getegid cannot fail; getgroups with a zero size only counts.
    let mut all = vec![unsafe { libc::getegid() }];
    let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if n > 0 {
        let mut buf = vec![0 as libc::gid_t; n as usize];
        // SAFETY: `buf` holds `n` entries.
        let got = unsafe { libc::getgroups(n, buf.as_mut_ptr()) };
        for g in buf.into_iter().take(got.max(0) as usize) {
            if !all.contains(&g) {
                all.push(g);
            }
        }
    }
    all
}

/// The group a file newly created in `dir` gets (the directory's on BSD, the creator's on
/// Linux), so a test can pick a different one and prove the copy carried it.
fn default_group_in(dir: &Path) -> u32 {
    let probe = dir.join(".group-probe");
    create_test_file(&probe, 0).unwrap();
    let gid = meta(&probe).gid();
    fs::remove_file(&probe).unwrap();
    gid
}

/// A group the caller belongs to other than `not`, if it has one.
fn another_group(not: u32) -> Option<u32> {
    my_groups().into_iter().find(|g| *g != not)
}

/// Give every source file the caller's own effective group, so carrying it cannot be refused
/// (a temp directory's group can be one the caller is not in).
fn own_group(fx: &Fixture) {
    // SAFETY: getegid cannot fail.
    let gid = unsafe { libc::getegid() };
    for path in list_files_recursive(&fx.source) {
        chgrp(&path, gid).unwrap();
    }
}

/// The lines after the blank line that opens the summary.
fn summary(stdout: &str) -> Vec<String> {
    let lines: Vec<&str> = stdout.lines().collect();
    let start = lines
        .iter()
        .rposition(|l| l.is_empty())
        .map_or(0, |i| i + 1);
    lines[start..].iter().map(|l| l.to_string()).collect()
}

#[test]
fn cp_carries_bytes_mode_atime_and_mtime() {
    let fx = Fixture::new(&[("u/a.bin", 5000), ("u/ro.txt", 10), ("u/x.sh", 7)]);
    let cases = [
        (
            "u/a.bin",
            0o640,
            (1_600_000_000, 123_456_789),
            (1_650_000_000, 987_654_321),
        ),
        (
            "u/ro.txt",
            0o444,
            (1_500_000_000, 1),
            (1_400_000_000, 999_999_999),
        ),
        (
            "u/x.sh",
            0o750,
            (1_700_000_000, 0),
            (1_700_000_100, 500_000_000),
        ),
    ];
    for (rel, mode, atime, mtime) in cases {
        chmod(&fx.src(rel), mode);
        set_times(&fx.src(rel), atime, mtime);
    }
    own_group(&fx);
    let fx = fx.build();
    let (out, err, ok) = run_cp(&fx.args(&["--force"]));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Copied: 3"], "{out}");
    for (rel, mode, atime, mtime) in cases {
        // Metadata first: reading the copy's bytes may move its atime.
        let d = meta(&fx.dest(rel));
        assert!(d.file_type().is_file(), "{rel}");
        assert_eq!(d.mode() & 0o7777, mode, "{rel}: mode");
        assert_eq!((d.atime(), d.atime_nsec()), atime, "{rel}: atime");
        assert_eq!((d.mtime(), d.mtime_nsec()), mtime, "{rel}: mtime");
        assert_eq!(
            fs::read(fx.dest(rel)).unwrap(),
            fs::read(fx.src(rel)).unwrap(),
            "{rel}: bytes"
        );
    }
}

#[test]
fn cp_carries_owner_and_group_when_permitted() {
    let fx = Fixture::new(&[("u/a.txt", 10), ("u/setid.bin", 12)]);
    // Prefer a group a new file under --into would not get by itself, so a carried group is
    // visible; fall back to the caller's own.
    // SAFETY: getegid cannot fail.
    let fallback = unsafe { libc::getegid() };
    let gid = another_group(default_group_in(&fx.into)).unwrap_or(fallback);
    for rel in ["u/a.txt", "u/setid.bin"] {
        chgrp(&fx.src(rel), gid).unwrap();
    }
    chmod(&fx.src("u/setid.bin"), 0o6755);
    let fx = fx.build();
    let (out, err, ok) = run_cp(&fx.args(&["--force"]));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Copied: 2"], "{out}");
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    for rel in ["u/a.txt", "u/setid.bin"] {
        let d = meta(&fx.dest(rel));
        assert_eq!(d.uid(), uid, "{rel}: owner");
        assert_eq!(d.gid(), gid, "{rel}: group");
    }
    // A sandbox can strip set-id bits on chmod; the source's own mode is what to carry.
    let source_mode = meta(&fx.src("u/setid.bin")).mode() & 0o7777;
    if source_mode & 0o6000 == 0 {
        eprintln!("this host strips set-id bits on chmod; the set-id check runs on CI");
    }
    assert_eq!(
        meta(&fx.dest("u/setid.bin")).mode() & 0o7777,
        source_mode,
        "set-id bits are kept when both ids were carried"
    );
}

/// One filter's case: the flag, how to make the file match before indexing, and how to make
/// it stop matching afterwards.
struct SafeCase {
    args: Vec<String>,
    flag: &'static str,
    before: Box<dyn Fn(&Path)>,
    after: Box<dyn Fn(&Path)>,
}

fn safe_cases() -> Vec<SafeCase> {
    let resize = |len: u64| {
        move |p: &Path| {
            fs::OpenOptions::new()
                .write(true)
                .open(p)
                .unwrap()
                .set_len(len)
                .unwrap()
        }
    };
    let mode = |m: u32| move |p: &Path| chmod(p, m);
    let atime = |d: u64| move |p: &Path| set_atime_days_ago(p, d).unwrap();
    let mtime = |d: u64| move |p: &Path| set_mtime_days_ago(p, d).unwrap();
    let case =
        |args: &[&str], flag, before: Box<dyn Fn(&Path)>, after: Box<dyn Fn(&Path)>| SafeCase {
            args: args.iter().map(|s| s.to_string()).collect(),
            flag,
            before,
            after,
        };
    let mut cases = vec![
        case(
            &["--min-size", "2K"],
            "--min-size",
            Box::new(resize(4096)),
            Box::new(resize(100)),
        ),
        case(
            &["--max-size", "64K"],
            "--max-size",
            Box::new(resize(100)),
            Box::new(resize(200_000)),
        ),
        case(
            &["--older-than", "30"],
            "--older-than",
            Box::new(atime(100)),
            Box::new(atime(0)),
        ),
        case(
            &["--newer-than", "30"],
            "--newer-than",
            Box::new(atime(0)),
            Box::new(atime(100)),
        ),
        case(
            &["--mtime-older-than", "30"],
            "--mtime-older-than",
            Box::new(mtime(100)),
            Box::new(mtime(0)),
        ),
        case(
            &["--mtime-newer-than", "30"],
            "--mtime-newer-than",
            Box::new(mtime(0)),
            Box::new(mtime(100)),
        ),
        case(
            &["--mode", "644"],
            "--mode",
            Box::new(mode(0o644)),
            Box::new(mode(0o640)),
        ),
        case(
            &["--mode", "/004"],
            "--mode",
            Box::new(mode(0o644)),
            Box::new(mode(0o640)),
        ),
        case(
            &["--mode", "&640"],
            "--mode",
            Box::new(mode(0o640)),
            Box::new(mode(0o600)),
        ),
    ];
    // A group change needs a second group the caller belongs to; `--owner` would need root to
    // change a file's owner, so its live check is proven by the library's boundary grid.
    let groups = my_groups();
    if groups.len() >= 2 {
        let (first, second) = (groups[0], groups[1]);
        cases.push(case(
            &["--group", &first.to_string()],
            "--group",
            Box::new(move |p: &Path| chgrp(p, first).unwrap()),
            Box::new(move |p: &Path| chgrp(p, second).unwrap()),
        ));
    } else {
        eprintln!("cp_safe: the caller has one group; the --group case runs where it has two");
    }
    cases
}

#[test]
fn cp_safe_skips_a_file_that_no_longer_matches_each_filter() {
    let cases = safe_cases();
    assert!(cases.len() >= 9);
    for case in cases {
        let fx = Fixture::new(&[("u/changed.dat", 10), ("u/kept.dat", 10)]);
        own_group(&fx);
        for rel in ["u/changed.dat", "u/kept.dat"] {
            (case.before)(&fx.src(rel));
        }
        let fx = fx.build();
        let mut args: Vec<&str> = case.args.iter().map(String::as_str).collect();
        args.push("-n");
        let (out, _, ok) = run_cp(&fx.args(&args));
        assert!(ok);
        assert_eq!(
            dry_run_sources(&out).len(),
            2,
            "{:?} selects both",
            case.args
        );

        (case.after)(&fx.src("u/changed.dat"));
        let mut args: Vec<&str> = case.args.iter().map(String::as_str).collect();
        args.extend(["--safe", "--force", "-v"]);
        let (out, err, ok) = run_cp(&fx.args(&args));
        assert!(ok, "{:?}: {err}", case.args);
        let line = format!(
            "skipped (no longer matches {}): {}",
            case.flag,
            fx.src("u/changed.dat").display()
        );
        assert!(out.contains(&line), "{:?}: {out}", case.args);
        assert_eq!(
            summary(&out),
            ["Copied: 1", "Skipped: 1"],
            "{:?}: {out}",
            case.args
        );
        assert_eq!(fx.copied(), ["u/kept.dat"], "{:?}", case.args);

        // Without --safe the stale index row still selects it, so the skip was the re-check's.
        args.retain(|a| *a != "--safe");
        let (out, _, _) = run_cp(&fx.args(&args));
        assert!(out.contains("Copied: 1"), "{:?}: {out}", case.args);
        assert_eq!(
            fx.copied(),
            ["u/changed.dat", "u/kept.dat"],
            "{:?}",
            case.args
        );
    }
}

/// An index built with `--block-size 1M` does not record that measure,
/// so `--safe` cannot recompute it: a file indexed at 900 KiB (one block, under
/// `--max-size 1500K`) and since grown to 1.2 MB (two blocks) passes both live measures. Its
/// mtime, moved since the crawl, is what skips it.
#[test]
fn cp_safe_skips_a_file_that_grew_past_the_bound_by_the_index_block_size() {
    let fx = Fixture::new(&[("u/grew.dat", 921_600), ("u/kept.dat", 102_400)]);
    for rel in ["u/grew.dat", "u/kept.dat"] {
        set_mtime_days_ago(&fx.src(rel), 10).unwrap();
    }
    build_index_block_size(&fx.source, &fx.index, "1M");
    let (out, err, ok) = run_cp(&fx.args(&["--max-size", "1500K", "-n"]));
    assert!(ok, "{err}");
    assert_eq!(dry_run_sources(&out).len(), 2, "{out}");

    append_bytes(&fx.src("u/grew.dat"), 307_200);
    let fresh = fx.index.with_file_name("fresh");
    build_index_block_size(&fx.source, &fresh, "1M");
    let kept = fx.src("u/kept.dat").display().to_string();
    assert_eq!(
        find_paths_with(&fresh, &["--max-size", "1500K"]),
        [kept],
        "by the index's own measure it no longer matches"
    );

    let (out, err, ok) = run_cp(&fx.args(&["--max-size", "1500K", "--safe", "--force", "-v"]));
    assert!(ok, "{err}");
    assert!(
        out.contains(&format!(
            "skipped (changed since the index was built): {}",
            fx.src("u/grew.dat").display()
        )),
        "{out}"
    );
    assert_eq!(summary(&out), ["Copied: 1", "Skipped: 1"], "{out}");
    assert_eq!(fx.copied(), ["u/kept.dat"]);
}

#[test]
fn cp_safe_keeps_a_file_that_still_matches() {
    let fx = Fixture::new(&[
        ("alice/a.log", 4096),
        ("alice/b.log", 6000),
        ("bob/c.txt", 3000),
    ]);
    for path in list_files_recursive(&fx.source) {
        chmod(&path, 0o644);
        set_atime_days_ago(&path, 100).unwrap();
        set_mtime_days_ago(&path, 100).unwrap();
    }
    own_group(&fx);
    let fx = fx.build();
    // SAFETY: getuid and getegid cannot fail.
    let uid = unsafe { libc::getuid() }.to_string();
    let gid = unsafe { libc::getegid() }.to_string();

    let every = [
        "-p",
        "*.log",
        "--min-size",
        "2K",
        "--max-size",
        "64K",
        "--older-than",
        "30",
        "--newer-than",
        "400",
        "--owner",
        &uid,
        "--group",
        &gid,
        "--mode",
        "644",
        "--mtime-older-than",
        "30",
        "--mtime-newer-than",
        "400",
        "--safe",
        "--force",
    ];
    let (out, err, ok) = run_cp(&fx.args(&every));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Copied: 2"], "{out}");
    assert_eq!(fx.copied(), ["alice/a.log", "alice/b.log"]);

    // With no filter, --safe still copies every regular file.
    let (out, err, ok) = run_cp(&fx.args(&["--safe", "--force"]));
    assert!(!ok, "the two copies above now exist: {err}");
    assert_eq!(
        summary(&out),
        ["Copied: 1", "Existing at destination: 2"],
        "{out}"
    );
    assert_eq!(fx.copied(), ["alice/a.log", "alice/b.log", "bob/c.txt"]);
}

#[test]
fn cp_safe_skips_a_file_replaced_by_a_symlink() {
    let fx = Fixture::new(&[("u/a.txt", 3000), ("u/b.txt", 3000)]).build();
    let lookalike = fx.source.with_file_name("lookalike.txt");
    create_test_file(&lookalike, 3000).unwrap();
    fs::remove_file(fx.src("u/b.txt")).unwrap();
    std::os::unix::fs::symlink(&lookalike, fx.src("u/b.txt")).unwrap();

    let (out, err, ok) = run_cp(&fx.args(&["--min-size", "2K", "--safe", "--force", "-v"]));
    assert!(ok, "{err}");
    assert!(
        out.contains(&format!(
            "skipped (not a regular file): {}",
            fx.src("u/b.txt").display()
        )),
        "{out}"
    );
    assert_eq!(summary(&out), ["Copied: 1", "Skipped: 1"], "{out}");
    assert_eq!(fx.copied(), ["u/a.txt"]);
    assert!(fs::symlink_metadata(fx.dest("u/b.txt")).is_err());
}

#[test]
fn cp_summary_counts_each_outcome_separately() {
    let fx = Fixture::new(&[
        ("u/ok1", 10),
        ("u/ok2", 10),
        ("u/gone", 10),
        ("u/exists", 10),
        ("u/fifo", 10),
        ("u/touched", 10),
        ("v/blocked", 10),
    ]);
    for path in list_files_recursive(&fx.source) {
        set_mtime_days_ago(&path, 100).unwrap();
    }
    own_group(&fx);
    let fx = fx.build();
    fs::remove_file(fx.src("u/gone")).unwrap();
    fs::create_dir_all(fx.dest("u")).unwrap();
    create_test_file(&fx.dest("u/exists"), 1).unwrap();
    fs::remove_file(fx.src("u/fifo")).unwrap();
    mkfifo(&fx.src("u/fifo"));
    set_mtime_days_ago(&fx.src("u/touched"), 0).unwrap();
    create_test_file(&fx.dest("v"), 1).unwrap();

    let (out, err, ok) = run_cp(&fx.args(&["--mtime-older-than", "30", "--safe", "--force"]));
    assert!(!ok, "a failure and an existing destination exit non-zero");
    assert_eq!(
        summary(&out),
        [
            "Copied: 2",
            "Missing: 1",
            "Existing at destination: 1",
            "Skipped: 2",
            "Failed: 1"
        ],
        "{out}"
    );
    assert!(
        err.contains(&format!("xdu-cp: {}:", fx.src("v/blocked").display())),
        "{err}"
    );
    assert_eq!(
        err.lines().filter(|l| l.starts_with("xdu-cp: ")).count(),
        1,
        "{err}"
    );

    // Missing and skipped files alone are not failures.
    let quiet = Fixture::new(&[("u/a", 10), ("u/b", 10)]).build();
    fs::remove_file(quiet.src("u/a")).unwrap();
    fs::remove_file(quiet.src("u/b")).unwrap();
    mkfifo(&quiet.src("u/b"));
    let (out, err, ok) = run_cp(&quiet.args(&["--force"]));
    assert!(ok, "{err}");
    assert_eq!(
        summary(&out),
        ["Copied: 0", "Missing: 1", "Skipped: 1"],
        "{out}"
    );
}

#[test]
fn cp_never_modifies_a_source() {
    let fx = Fixture::new(&[("u/a.bin", 70_000), ("u/b.txt", 10), ("v/c.txt", 10)]);
    chmod(&fx.src("u/a.bin"), 0o4750);
    set_times(&fx.src("u/b.txt"), (1_600_000_000, 5), (1_600_000_000, 7));
    own_group(&fx);
    let fx = fx.build();
    fs::create_dir_all(fx.dest("u")).unwrap();
    create_test_file(&fx.dest("u/b.txt"), 1).unwrap();
    create_test_file(&fx.dest("v"), 1).unwrap();

    let snapshot = |rel: &str| {
        let m = meta(&fx.src(rel));
        (
            (m.ino(), m.nlink(), m.len(), m.mode(), m.uid(), m.gid()),
            (m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec()),
            fs::read(fx.src(rel)).unwrap(),
        )
    };
    let rels = ["u/a.bin", "u/b.txt", "v/c.txt"];
    let before: Vec<_> = rels.iter().map(|r| snapshot(r)).collect();
    for extra in [vec!["--force"], vec!["--safe", "--force", "-v"]] {
        let (out, _, ok) = run_cp(&fx.args(&extra));
        assert!(!ok, "an existing destination and a failure: {out}");
        let after: Vec<_> = rels.iter().map(|r| snapshot(r)).collect();
        assert_eq!(after, before, "{extra:?}: a source changed");
    }
    assert_eq!(fs::read(fx.dest("u/a.bin")).unwrap(), before[0].2);
    assert_eq!(fs::read(fx.dest("u/b.txt")).unwrap().len(), 1);
}

#[test]
fn cp_keeps_diagnostics_off_stdout() {
    let fx = Fixture::new(&[("u/a.txt", 10), ("u/fifo", 10), ("v/blocked", 10)]);
    own_group(&fx);
    let fx = fx.build();
    // A marker recording tolerated errors: the index is read, with a warning.
    let marker = fx.index.join(".xdu-complete");
    let body = fs::read_to_string(&marker).unwrap();
    assert!(body.contains("errors=0\n"), "{body}");
    fs::write(&marker, body.replace("errors=0\n", "errors=2\n")).unwrap();
    fs::remove_file(fx.src("u/fifo")).unwrap();
    mkfifo(&fx.src("u/fifo"));
    create_test_file(&fx.dest("v"), 1).unwrap();

    let (out, err, ok) = run_cp(&fx.args(&["--force", "--safe"]));
    assert!(!ok);
    assert_eq!(
        out, "\nCopied: 1\nSkipped: 1\nFailed: 1\n",
        "stdout holds the summary alone; stderr: {err}"
    );
    assert!(err.contains("2 tolerated error(s)"), "{err}");
    assert!(
        err.contains(&format!("xdu-cp: {}: ", fx.src("v/blocked").display())),
        "{err}"
    );

    let (out, err, _) = run_with_stdin("xdu-cp", &fx.args(&["-v"]), Some("y\n"));
    assert!(err.contains("[y/N] "), "{err}");
    for line in out.lines().filter(|l| !l.is_empty()) {
        let listing = ["copied", "existing:", "missing:", "skipped"]
            .iter()
            .any(|p| line.starts_with(p));
        let counted =
            line.contains(": ") && line.split(": ").nth(1).unwrap().parse::<u64>().is_ok();
        assert!(listing || counted, "unexpected stdout line {line:?}");
    }
    assert!(!out.contains("not a directory"), "{out}");
    assert!(!out.contains("warning"), "{out}");
}

#[test]
fn cp_verbose_lists_in_path_order() {
    let mut names = Vec::new();
    for p in ["c", "a", "b"] {
        for f in ["z", "m", "a", "q", "k", "b", "y", "d", "x", "e"] {
            names.push(format!("{p}/{f}.txt"));
        }
    }
    let files: Vec<(&str, usize)> = names.iter().map(|n| (n.as_str(), 16)).collect();
    let fx = Fixture::new(&files);
    own_group(&fx);
    let fx = fx.build();
    let (out, err, ok) = run_cp(&fx.args(&["--force", "-v", "-j", "8"]));
    assert!(ok, "{err}");
    let listed: Vec<String> = out
        .lines()
        .filter_map(|l| l.strip_prefix("copied: "))
        .map(|l| l.split_once(" -> ").unwrap().0.to_string())
        .collect();
    let mut expected: Vec<String> = names
        .iter()
        .map(|n| fx.src(n).display().to_string())
        .collect();
    expected.sort();
    assert_eq!(listed, expected);
    for line in out.lines().filter(|l| l.starts_with("copied: ")) {
        let (src, dest) = line["copied: ".len()..].split_once(" -> ").unwrap();
        let rel = Path::new(src).strip_prefix(&fx.source).unwrap();
        assert_eq!(Path::new(dest), fx.into.join(rel));
    }
    assert_eq!(summary(&out), ["Copied: 30"]);
}

// ---- Directory permissions ----

/// Permission bits bind every caller but root, so these cases run as any other user.
fn running_as_root() -> bool {
    // SAFETY: geteuid cannot fail.
    unsafe { libc::geteuid() == 0 }
}

/// Gives each directory back mode 0755 when dropped, so a failed assertion still lets the
/// temp directory be removed.
struct Unlock(Vec<PathBuf>);

impl Drop for Unlock {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
        }
    }
}

/// Mode 0111 gives its owner what 0711 gives every other user: search, no listing. `cp`(1)
/// copies through such an ancestor, and so does `xdu-cp`, on both sides. A copy never flushes
/// its source directory, so that one need not be readable either (0311).
#[test]
fn cp_copies_through_search_only_ancestors() {
    if running_as_root() {
        eprintln!("skipped: root ignores permission bits; this runs as any other user");
        return;
    }
    let fx = Fixture::new(&[("u/sub/a.dat", 1), ("u/sub/b.dat", 2)]).build();
    fs::create_dir_all(fx.dest("u/sub")).unwrap();
    let unlock = Unlock(vec![fx.src("u"), fx.src("u/sub"), fx.dest("u")]);
    chmod(&fx.src("u"), 0o111);
    chmod(&fx.src("u/sub"), 0o311);
    chmod(&fx.dest("u"), 0o111);

    let (out, err, ok) = run_cp(&fx.args(&["--force"]));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Copied: 2"], "{out}");
    drop(unlock);
    assert_eq!(fx.copied(), ["u/sub/a.dat", "u/sub/b.dat"]);
}

/// A destination directory is flushed after each publish, which needs read permission. One
/// the caller may write but not read (0311) fails each file bound there, naming the
/// directory, and nothing is written in it; the other files are copied.
#[test]
fn cp_fails_closed_on_a_destination_directory_it_cannot_read() {
    if running_as_root() {
        eprintln!("skipped: root ignores permission bits; this runs as any other user");
        return;
    }
    let fx = Fixture::new(&[("u/a.dat", 1), ("v/b.dat", 2)]).build();
    fs::create_dir(fx.dest("u")).unwrap();
    let unlock = Unlock(vec![fx.dest("u")]);
    chmod(&fx.dest("u"), 0o311);

    let (out, err, ok) = run_cp(&fx.args(&["--force"]));
    assert!(!ok, "a failed file exits non-zero");
    let line = format!(
        "xdu-cp: {}: cannot open directory u for reading (",
        fx.src("u/a.dat").display()
    );
    assert!(err.contains(&line), "{err}");
    assert!(
        err.contains("which flushing it to stable storage needs"),
        "{err}"
    );
    assert_eq!(summary(&out), ["Copied: 1", "Failed: 1"], "{out}");
    drop(unlock);
    assert_eq!(fx.copied(), ["v/b.dat"]);
    assert_eq!(fs::read_dir(fx.dest("u")).unwrap().count(), 0);
}
