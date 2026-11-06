//! Integration tests for `xdu-cp`, driving the real binary against throwaway indexes.
//!
//! Index rows hold the canonical crawl root, so every fixture works in canonical paths and
//! passes `--root` as the canonical source tree (on macOS a `TempDir` under `/var` is indexed
//! as `/private/var`). Indexes use `--apparent-size` so size filters are exact.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use common::{
    binary_path, build_index, create_test_file, list_files_recursive, run_cp, run_find,
    run_with_stdin, set_atime_days_ago, set_mtime_days_ago,
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
