//! Integration tests for `xdu-mv` on one filesystem, driving the real binary against
//! throwaway indexes.
//!
//! The source tree and `--into` live in one temp directory, so every move is a rename on one
//! filesystem. Index rows hold the canonical crawl root, so every fixture works in canonical
//! paths and passes `--root` as the canonical source tree (on macOS a `TempDir` under `/var`
//! is indexed as `/private/var`). Indexes use `--apparent-size` so size filters are exact.

mod common;

use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use common::{
    binary_path, build_index, create_test_file, list_files_recursive, run_find, run_mv,
    run_with_stdin, set_atime_days_ago, set_mtime_days_ago,
};

struct Fixture {
    _dir: TempDir,
    base: PathBuf,
    source: PathBuf,
    index: PathBuf,
    into: PathBuf,
}

impl Fixture {
    /// A source tree of `files` (relative path, size), not yet indexed, and an empty `--into`
    /// beside it on the same filesystem.
    fn new(files: &[(&str, usize)]) -> Fixture {
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
            index: base.join("index"),
            base,
            source,
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
    fn moved(&self) -> Vec<String> {
        relative_files(&self.into)
    }

    /// Every file left under the source tree, as paths relative to it.
    fn left(&self) -> Vec<String> {
        relative_files(&self.source)
    }

    /// Nothing at all under `--into`: no file, no directory.
    fn nothing_under_into(&self) -> bool {
        fs::read_dir(&self.into).unwrap().next().is_none()
    }
}

fn relative_files(dir: &Path) -> Vec<String> {
    list_files_recursive(dir)
        .iter()
        .map(|p| p.strip_prefix(dir).unwrap().to_str().unwrap().to_string())
        .collect()
}

fn meta(path: &Path) -> fs::Metadata {
    fs::symlink_metadata(path).unwrap()
}

/// The sources a dry-run lists, in its order.
fn dry_run_sources(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| l.split_once(" -> ").map(|(src, _)| src.to_string()))
        .collect()
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

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn mv_renames_on_one_filesystem_keeping_the_inode() {
    let fx = Fixture::new(&[
        ("alice/logs/app.log", 2048),
        ("alice/data.bin", 4096),
        ("bob/notes.txt", 1024),
    ]);
    chmod(&fx.src("alice/data.bin"), 0o640);
    set_mtime_days_ago(&fx.src("alice/data.bin"), 50).unwrap();
    let fx = fx.build();
    let before: Vec<fs::Metadata> = ["alice/logs/app.log", "alice/data.bin"]
        .iter()
        .map(|r| meta(&fx.src(r)))
        .collect();

    let (out, err, ok) = run_mv(&fx.args(&["-u", "alice", "--force", "-v"]));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Moved: 2"], "{out}");
    for rel in ["alice/data.bin", "alice/logs/app.log"] {
        assert!(
            out.contains(&format!(
                "moved: {} -> {}",
                fx.src(rel).display(),
                fx.dest(rel).display()
            )),
            "{out}"
        );
    }
    assert_eq!(fx.moved(), ["alice/data.bin", "alice/logs/app.log"]);
    assert_eq!(fx.left(), ["bob/notes.txt"]);
    for (rel, was) in ["alice/logs/app.log", "alice/data.bin"].iter().zip(&before) {
        let now = meta(&fx.dest(rel));
        assert_eq!((now.dev(), now.ino()), (was.dev(), was.ino()), "{rel}");
        assert_eq!(now.mode(), was.mode(), "{rel}");
        assert_eq!((now.uid(), now.gid()), (was.uid(), was.gid()), "{rel}");
        assert_eq!(
            (now.mtime(), now.mtime_nsec()),
            (was.mtime(), was.mtime_nsec())
        );
        assert_eq!(now.len(), was.len(), "{rel}");
    }

    // Without --root the whole absolute path is reproduced under --into; XDU_INDEX and
    // XDU_JOBS stand in for -i and -j.
    let whole_into = fx.base.join("whole");
    fs::create_dir(&whole_into).unwrap();
    let inode = meta(&fx.src("bob/notes.txt")).ino();
    let output = Command::new(binary_path("xdu-mv"))
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
    assert_eq!(meta(&mirrored).ino(), inode);
    assert_eq!(list_files_recursive(&whole_into), vec![mirrored]);
    assert!(fx.left().is_empty());
}

#[test]
fn mv_never_replaces_an_existing_destination() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("alice/b.txt", 20), ("bob/c.txt", 30)]).build();
    fs::create_dir_all(fx.dest("alice")).unwrap();
    create_test_file(&fx.dest("alice/b.txt"), 3).unwrap();
    let (out, err, ok) = run_mv(&fx.args(&["--force", "-v"]));
    assert!(!ok, "an existing destination must exit non-zero");
    assert_eq!(
        summary(&out),
        ["Moved: 2", "Existing at destination: 1"],
        "{out}"
    );
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
    assert_eq!(
        fs::read(fx.src("alice/b.txt")).unwrap().len(),
        20,
        "the source of a skipped move stays"
    );
    assert_eq!(fs::read(fx.dest("alice/a.txt")).unwrap().len(), 10);
    assert_eq!(fs::read(fx.dest("bob/c.txt")).unwrap().len(), 30);
    assert_eq!(fx.left(), ["alice/b.txt"]);

    // A re-run finds the moved files gone and the one left still blocked.
    let (out, _, ok) = run_mv(&fx.args(&["--force"]));
    assert!(!ok);
    assert_eq!(
        summary(&out),
        ["Moved: 0", "Missing: 2", "Existing at destination: 1"],
        "{out}"
    );
    assert_eq!(fs::read(fx.dest("alice/b.txt")).unwrap().len(), 3);
}

#[test]
fn mv_dry_run_writes_nothing() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("bob/deep/b.txt", 20)]).build();
    let (out, err, ok) = run_mv(&fx.args(&["--dry-run"]));
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
            "2 file(s) would be moved into {}",
            fx.into.display()
        )),
        "{out}"
    );
    assert!(
        fx.nothing_under_into(),
        "a dry run created something under --into"
    );
    assert_eq!(fx.left(), ["alice/a.txt", "bob/deep/b.txt"]);
}

#[test]
fn mv_dry_run_and_the_real_run_select_identical_rows_under_limit() {
    let names = ["u/f", "u/c", "u/e", "u/a", "u/d", "u/b"];
    let files: Vec<(&str, usize)> = names.iter().map(|n| (*n, 8)).collect();
    let fx = Fixture::new(&files).build();
    let (out, err, ok) = run_mv(&fx.args(&["-l", "3", "-n"]));
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

    let (out, err, ok) = run_mv(&fx.args(&["-l", "3", "--force"]));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Moved: 3"], "{out}");
    let moved: Vec<String> = fx
        .moved()
        .iter()
        .map(|r| fx.src(r).display().to_string())
        .collect();
    assert_eq!(moved, previewed);
    assert_eq!(fx.left(), ["u/d", "u/e", "u/f"]);
}

/// The S3 refusal precedes everything: a garbage `httpfs` in `XDU_EXTENSION_DIR` would have
/// printed its own failure had any extension loaded, and a selection would have printed to
/// stdout. The one refusal line, and nothing written, is the proof none ran.
#[test]
fn mv_refuses_an_s3_index_before_anything() {
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
        let mut cmd = hermetic.command("xdu-mv");
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
        let output = cmd.output().expect("spawn xdu-mv");
        assert_eq!(output.status.code(), Some(1), "via_env={via_env}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "xdu-mv: S3 indices are not supported by xdu-mv\n",
            "via_env={via_env}"
        );
        assert!(output.stdout.is_empty(), "via_env={via_env}");
    }
    assert_eq!(fs::read_dir(into.path()).unwrap().count(), 0);
    hermetic.assert_left_empty();
}

#[test]
fn mv_refuses_a_path_outside_root_before_moving_anything() {
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
        let (out, err, ok) = run_mv(&args);
        assert!(!ok, "{extra:?}");
        let first = fx.src("bob/b1.txt").display().to_string();
        assert!(
            err.contains(&format!(
                "refusing to move anything: {first} is not under --root"
            )),
            "{err}"
        );
        assert!(!err.contains("b2.txt") && !err.contains("carol"), "{err}");
        assert!(!out.contains(" -> "), "{out}");
        assert!(fx.nothing_under_into());
        assert_eq!(
            fx.left(),
            ["alice/a.txt", "bob/b1.txt", "bob/b2.txt", "carol/c.txt"],
            "{extra:?}: alice/a.txt maps cleanly, and still did not move"
        );
    }

    // A destination equal to its own source refuses the same way.
    let (_, err, ok) = run_mv(&[
        "-i",
        Fixture::s(&fx.index),
        "--into",
        Fixture::s(&fx.source),
        "--root",
        Fixture::s(&fx.source),
        "--force",
    ]);
    assert!(!ok);
    assert!(
        err.contains(&format!(
            "{} maps to a destination equal to its own source path",
            fx.src("alice/a.txt").display()
        )),
        "{err}"
    );
    assert_eq!(fx.left().len(), 4);
}

#[test]
fn mv_refuses_a_missing_into() {
    let fx = Fixture::new(&[("alice/a.txt", 10)]).build();
    let missing = fx.into.join("absent");
    let (out, err, ok) = run_mv(&[
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
    let (out, err, ok) = run_mv(&[
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
    assert_eq!(fx.left(), ["alice/a.txt"]);
}

#[test]
fn mv_declined_prompt_moves_nothing() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("bob/b.txt", 10)]).build();
    for input in [Some("n\n"), Some("\n"), Some("yep\n"), None] {
        let (out, err, ok) = run_with_stdin("xdu-mv", &fx.args(&[]), input);
        assert!(ok, "{input:?}: {err}");
        assert!(
            err.contains(&format!(
                "Move 2 file(s) into {}? [y/N] ",
                fx.into.display()
            )),
            "{input:?}: {err}"
        );
        assert!(err.contains("Aborted."), "{input:?}: {err}");
        assert!(!out.contains("Moved"), "{input:?}: {out}");
        assert!(fx.nothing_under_into(), "{input:?}: something was written");
        assert_eq!(fx.left(), ["alice/a.txt", "bob/b.txt"], "{input:?}");
    }

    let (out, err, ok) = run_with_stdin("xdu-mv", &fx.args(&[]), Some(" YES \n"));
    assert!(ok, "{err}");
    assert!(!out.contains("[y/N]"), "the prompt is on stderr: {out}");
    assert_eq!(summary(&out), ["Moved: 2"], "{out}");
    assert_eq!(fx.moved(), ["alice/a.txt", "bob/b.txt"]);
    assert!(fx.left().is_empty());
}

#[test]
fn mv_empty_match_exits_zero() {
    let fx = Fixture::new(&[("alice/a.txt", 10)]).build();
    let (out, err, ok) = run_mv(&fx.args(&["-p", "*.nomatch", "--force"]));
    assert!(ok, "{err}");
    assert_eq!(out.trim(), "No matching files found.");
    assert!(fx.nothing_under_into());
    assert_eq!(fx.left(), ["alice/a.txt"]);
}

/// One filter at a time: the file is selected, changed so the filter no longer holds, and
/// `--safe` leaves it at its source while its unchanged sibling moves. Without `--safe` the
/// stale row still moves it, so the skip was the re-check's.
#[test]
fn mv_safe_skips_a_changed_file_and_leaves_it() {
    type Change = fn(&Path);
    let cases: [(&[&str], &str, Change, Change); 4] = [
        (
            &["--min-size", "2K"],
            "--min-size",
            |p| {
                fs::File::options()
                    .write(true)
                    .open(p)
                    .unwrap()
                    .set_len(4096)
                    .unwrap()
            },
            |p| {
                fs::File::options()
                    .write(true)
                    .open(p)
                    .unwrap()
                    .set_len(100)
                    .unwrap()
            },
        ),
        (
            &["--older-than", "30"],
            "--older-than",
            |p| set_atime_days_ago(p, 100).unwrap(),
            |p| set_atime_days_ago(p, 0).unwrap(),
        ),
        (
            &["--mtime-older-than", "30"],
            "--mtime-older-than",
            |p| set_mtime_days_ago(p, 100).unwrap(),
            |p| set_mtime_days_ago(p, 0).unwrap(),
        ),
        (
            &["--mode", "644"],
            "--mode",
            |p| chmod(p, 0o644),
            |p| chmod(p, 0o600),
        ),
    ];
    for (filter, flag, before, after) in cases {
        let fx = Fixture::new(&[("u/changed.dat", 10), ("u/kept.dat", 10)]);
        for rel in ["u/changed.dat", "u/kept.dat"] {
            before(&fx.src(rel));
        }
        let fx = fx.build();
        let mut args = filter.to_vec();
        args.push("-n");
        let (out, _, ok) = run_mv(&fx.args(&args));
        assert!(ok);
        assert_eq!(dry_run_sources(&out).len(), 2, "{filter:?} selects both");

        after(&fx.src("u/changed.dat"));
        let inode = meta(&fx.src("u/changed.dat")).ino();
        let mut args = filter.to_vec();
        args.extend(["--safe", "--force", "-v"]);
        let (out, err, ok) = run_mv(&fx.args(&args));
        assert!(ok, "{filter:?}: {err}");
        assert!(
            out.contains(&format!(
                "skipped (no longer matches {flag}): {}",
                fx.src("u/changed.dat").display()
            )),
            "{filter:?}: {out}"
        );
        assert_eq!(summary(&out), ["Moved: 1", "Skipped: 1"], "{filter:?}");
        assert_eq!(fx.moved(), ["u/kept.dat"], "{filter:?}");
        assert_eq!(fx.left(), ["u/changed.dat"], "{filter:?}");
        assert_eq!(meta(&fx.src("u/changed.dat")).ino(), inode, "{filter:?}");

        args.retain(|a| *a != "--safe");
        let (out, _, _) = run_mv(&fx.args(&args));
        assert!(out.contains("Moved: 1"), "{filter:?}: {out}");
        assert_eq!(fx.moved(), ["u/changed.dat", "u/kept.dat"], "{filter:?}");
    }
}

#[test]
fn mv_skips_a_symlink_at_the_source_path() {
    let fx = Fixture::new(&[("alice/a.txt", 10), ("alice/link.txt", 10)]).build();
    let outside = fx.base.join("outside.txt");
    create_test_file(&outside, 77).unwrap();
    fs::remove_file(fx.src("alice/link.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, fx.src("alice/link.txt")).unwrap();

    let (out, err, ok) = run_mv(&fx.args(&["--force", "-v"]));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Moved: 1", "Skipped: 1"], "{out}");
    assert!(
        out.contains(&format!(
            "skipped (not a regular file): {}",
            fx.src("alice/link.txt").display()
        )),
        "{out}"
    );
    assert_eq!(fx.moved(), ["alice/a.txt"]);
    assert!(meta(&fx.src("alice/link.txt")).file_type().is_symlink());
    assert!(fs::symlink_metadata(fx.dest("alice/link.txt")).is_err());
    assert_eq!(fs::read(&outside).unwrap().len(), 77);
}

/// No `--safe`: a directory at a selected path would carry its whole subtree with it, so the
/// regular-file rule must hold on every run.
#[test]
fn mv_skips_a_directory_at_the_source_path() {
    let fx = Fixture::new(&[("u/a.txt", 10), ("u/dir", 10)]).build();
    fs::remove_file(fx.src("u/dir")).unwrap();
    fs::create_dir(fx.src("u/dir")).unwrap();
    create_test_file(&fx.src("u/dir/inner.txt"), 4).unwrap();

    let (out, err, ok) = run_mv(&fx.args(&["--force", "-v"]));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Moved: 1", "Skipped: 1"], "{out}");
    assert!(
        out.contains(&format!(
            "skipped (not a regular file): {}",
            fx.src("u/dir").display()
        )),
        "{out}"
    );
    assert_eq!(fx.moved(), ["u/a.txt"]);
    assert!(meta(&fx.src("u/dir")).is_dir());
    assert_eq!(fs::read(fx.src("u/dir/inner.txt")).unwrap().len(), 4);
    assert!(fs::symlink_metadata(fx.dest("u/dir")).is_err());
}

/// No `--safe`: a FIFO at a selected path stays, and is never opened.
#[test]
fn mv_skips_a_fifo_at_the_source_path() {
    let fx = Fixture::new(&[("u/a.txt", 10), ("u/fifo", 10)]).build();
    fs::remove_file(fx.src("u/fifo")).unwrap();
    mkfifo(&fx.src("u/fifo"));

    let (out, err, ok) = run_mv(&fx.args(&["--force", "-v"]));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Moved: 1", "Skipped: 1"], "{out}");
    assert!(
        out.contains(&format!(
            "skipped (not a regular file): {}",
            fx.src("u/fifo").display()
        )),
        "{out}"
    );
    assert_eq!(fx.moved(), ["u/a.txt"]);
    assert!(meta(&fx.src("u/fifo")).file_type().is_fifo());
    assert!(fs::symlink_metadata(fx.dest("u/fifo")).is_err());
}

#[test]
fn mv_summary_counts_and_exit_status() {
    let fx = Fixture::new(&[
        ("u/ok1", 10),
        ("u/ok2", 10),
        ("u/gone", 10),
        ("u/exists", 10),
        ("u/fifo", 10),
        ("v/blocked", 10),
    ])
    .build();
    fs::remove_file(fx.src("u/gone")).unwrap();
    fs::create_dir_all(fx.dest("u")).unwrap();
    create_test_file(&fx.dest("u/exists"), 1).unwrap();
    fs::remove_file(fx.src("u/fifo")).unwrap();
    mkfifo(&fx.src("u/fifo"));
    // A regular file where the replica needs the directory `v`.
    create_test_file(&fx.dest("v"), 1).unwrap();

    let (out, err, ok) = run_mv(&fx.args(&["--force"]));
    assert!(!ok, "a failure and an existing destination exit non-zero");
    assert_eq!(
        summary(&out),
        [
            "Moved: 2",
            "Missing: 1",
            "Existing at destination: 1",
            "Skipped: 1",
            "Failed: 1"
        ],
        "{out}"
    );
    let line = format!(
        "xdu-mv: {}: destination component v is not a directory",
        fx.src("v/blocked").display()
    );
    assert!(err.contains(&line), "{err}");
    assert_eq!(
        err.lines().filter(|l| l.starts_with("xdu-mv: ")).count(),
        1,
        "{err}"
    );
    assert!(!out.contains("not a directory"), "failures stay off stdout");
    assert_eq!(
        out.lines().next(),
        Some(""),
        "without -v stdout holds the summary alone: {out}"
    );
    assert_eq!(fx.left(), ["u/exists", "u/fifo", "v/blocked"]);

    // Missing and skipped files alone are not failures.
    let quiet = Fixture::new(&[("u/a", 10), ("u/b", 10)]).build();
    fs::remove_file(quiet.src("u/a")).unwrap();
    fs::remove_file(quiet.src("u/b")).unwrap();
    mkfifo(&quiet.src("u/b"));
    let (out, err, ok) = run_mv(&quiet.args(&["--force"]));
    assert!(ok, "{err}");
    assert_eq!(
        summary(&out),
        ["Moved: 0", "Missing: 1", "Skipped: 1"],
        "{out}"
    );

    // An existing destination alone exits non-zero.
    let blocked = Fixture::new(&[("u/a", 10)]).build();
    fs::create_dir_all(blocked.dest("u")).unwrap();
    create_test_file(&blocked.dest("u/a"), 1).unwrap();
    let (out, _, ok) = run_mv(&blocked.args(&["--force"]));
    assert!(!ok);
    assert_eq!(
        summary(&out),
        ["Moved: 0", "Existing at destination: 1"],
        "{out}"
    );
}

/// Each `xdu-find` filter selects the same set through `xdu-mv` as through `xdu-find`, alone
/// and all together.
#[test]
fn mv_accepts_every_xdu_find_filter() {
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
        chmod(&fx.src(rel), mode);
    }
    let fx = fx.build();
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() }.to_string();
    let gid = meta(&fx.src("alice/old.log")).gid().to_string();

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
    assert_eq!(find_paths_with(&fx.index, &[]).len(), 5);
    for set in &sets {
        let expected = find_paths_with(&fx.index, set);
        assert!(
            !expected.is_empty(),
            "{set:?} selects nothing; the case is vacuous"
        );
        let mut args = set.clone();
        args.push("-n");
        let (out, err, ok) = run_mv(&fx.args(&args));
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

    // The full filter set really moves exactly that file.
    let mut args = all.clone();
    args.push("--force");
    let (out, err, ok) = run_mv(&fx.args(&args));
    assert!(ok, "{err}");
    assert_eq!(summary(&out), ["Moved: 1"], "{out}");
    assert_eq!(fx.moved(), ["alice/old.log"]);
    assert_eq!(fx.left().len(), 4);
}
