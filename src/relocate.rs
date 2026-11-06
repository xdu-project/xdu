//! The act half of select-then-act for tools that place files under a destination tree.
//!
//! A selection's rows are mapped under `--into` by stripping `--root`, the whole mapped set is
//! checked before the first write, the operator previews or confirms, and a local pool acts on
//! the rows in path-ordered chunks while the main thread prints and counts each outcome in
//! order. Refusals name the first offending path in that order.

use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use rayon::prelude::*;

use crate::atomic_fs::{
    self, Crossed, DirHandle, Faults, Moved, NoHook, ParentCache, Placed, Source, Stat, Stop,
    components, copy_atomic, is_not_a_directory,
};
use crate::cli::{SelectArgs, XduCpArgs, XduMvArgs};
use crate::select::{Live, Row, Verdict, holds_live, select};
use crate::{QueryFilters, SizeMode};

/// Rows handed to the pool at a time. Each chunk's outcomes come back in index order, so
/// output streams and stays deterministic while the pool works.
const CHUNK: usize = 1024;

/// The `--root` → `--into` mapping, over canonical paths.
#[derive(Clone, Debug)]
pub struct RootMap {
    root: PathBuf,
    into: PathBuf,
}

/// A selected path the run refuses to act on, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub path: String,
    pub reason: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.path, self.reason)
    }
}

impl RootMap {
    /// Both paths must already be canonical: index rows hold the canonical crawl root, so on
    /// macOS a `--root` under `/var` only matches once it reads `/private/var`.
    pub fn new(root: PathBuf, into: PathBuf) -> RootMap {
        RootMap { root, into }
    }

    pub fn into_dir(&self) -> &Path {
        &self.into
    }

    /// The path of `source` relative to `--root`, which is also its path relative to `--into`.
    ///
    /// The row must be absolute and normalized, with no empty, `.` or `..` component, so a
    /// hand-edited index cannot climb out of `--into`; and it must lie under `--root` component
    /// by component, so `/data2/f` is not under `/data`.
    pub fn map(&self, source: &str) -> Result<PathBuf, Refusal> {
        let refuse = |reason: String| Refusal {
            path: source.to_string(),
            reason,
        };
        let normalized = source.strip_prefix('/').is_some_and(|rest| {
            rest.split('/')
                .all(|c| !c.is_empty() && c != "." && c != "..")
        });
        if !normalized {
            return Err(refuse("is not an absolute, normalized path".to_string()));
        }
        let rel = match Path::new(source).strip_prefix(&self.root) {
            Ok(rel) if !rel.as_os_str().is_empty() => rel.to_path_buf(),
            _ => {
                return Err(refuse(format!(
                    "is not under --root {}",
                    self.root.display()
                )));
            }
        };
        if self.into.join(&rel) == Path::new(source) {
            return Err(refuse(
                "maps to a destination equal to its own source path".to_string(),
            ));
        }
        Ok(rel)
    }

    pub fn destination(&self, rel: &Path) -> PathBuf {
        self.into.join(rel)
    }
}

/// One selected file and where it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub source: PathBuf,
    pub rel: PathBuf,
}

/// Map every row before anything is written, refusing on the first row, in path order, that
/// is outside `--root`, not normalized, or would land on itself.
pub fn check_whole_set(map: &RootMap, rows: &[Row]) -> Result<Vec<Planned>, Refusal> {
    rows.iter()
        .map(|row| {
            map.map(&row.path).map(|rel| Planned {
                source: PathBuf::from(&row.path),
                rel,
            })
        })
        .collect()
}

/// Ask on stderr and read one line: only `y` or `yes`, in any case, proceeds.
///
/// The question goes to stderr so a `> log` redirect of the listing and summary cannot hide
/// it, as `cp -i` asks. End of input is a refusal.
pub fn confirm(question: &str, input: &mut dyn BufRead, err: &mut dyn Write) -> io::Result<bool> {
    write!(err, "{question} [y/N] ")?;
    err.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    let answer = answer.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}

/// What happened to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done { owner_kept: bool },
    Missing,
    Existing,
    Skipped(&'static str),
    Failed(String),
}

/// The counted outcomes of a run; each kind is its own count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub done: u64,
    pub missing: u64,
    pub existing: u64,
    pub skipped: u64,
    pub owner_not_preserved: u64,
    pub failed: u64,
}

impl Summary {
    pub fn add(&mut self, outcome: &Outcome) {
        match outcome {
            Outcome::Done { owner_kept } => {
                self.done += 1;
                if !owner_kept {
                    self.owner_not_preserved += 1;
                }
            }
            Outcome::Missing => self.missing += 1,
            Outcome::Existing => self.existing += 1,
            Outcome::Skipped(_) => self.skipped += 1,
            Outcome::Failed(_) => self.failed += 1,
        }
    }

    /// The summary lines: the action count always, every other count when it is not zero.
    pub fn lines(&self, verb: &str) -> Vec<String> {
        let mut lines = vec![format!("{verb}: {}", self.done)];
        for (label, n) in [
            ("Missing", self.missing),
            ("Existing at destination", self.existing),
            ("Skipped", self.skipped),
            ("Ownership not preserved", self.owner_not_preserved),
            ("Failed", self.failed),
        ] {
            if n > 0 {
                lines.push(format!("{label}: {n}"));
            }
        }
        lines
    }

    /// Non-zero when a file failed or its destination already existed: either way the
    /// operator asked for a file to be placed and it was not.
    pub fn exit_code(&self) -> i32 {
        if self.failed > 0 || self.existing > 0 {
            1
        } else {
            0
        }
    }
}

/// A worker's walk caches: the last source parent and the last destination parent.
#[derive(Debug, Default)]
struct Worker {
    source_parent: ParentCache,
    dest_parent: ParentCache,
}

/// The descriptors every worker shares: `/` for source walks and the canonical `--into`.
struct Anchors {
    root: DirHandle,
    into: DirHandle,
}

/// What `--safe` re-checks each file against: the filters the rows were selected by, and the
/// size measure the index records, if it records one.
#[derive(Clone, Copy, Debug)]
pub struct SafeCheck<'a> {
    pub filters: &'a QueryFilters,
    pub size_mode: Option<SizeMode>,
}

/// The `--safe` view of an object's `lstat`-semantics metadata.
fn live(stat: &Stat) -> Live {
    Live {
        regular: stat.is_regular(),
        length: stat.size,
        disk_usage: stat.blocks.saturating_mul(512),
        uid: stat.uid,
        gid: stat.gid,
        mode: stat.mode & 0o7777,
        atime: stat.atime,
        mtime: stat.mtime,
    }
}

/// Copy one planned file: walk to its source without following a symlink, open it only if it
/// is a regular file, re-check it under `--safe` against the `fstat` of the very descriptor
/// the copy reads, walk to (creating) its destination directory, and place it atomically.
fn copy_one(
    anchors: &Anchors,
    worker: &mut Worker,
    item: &Planned,
    safe: Option<SafeCheck<'_>>,
) -> Outcome {
    let (Some(parent), Some(name)) = (item.source.parent(), item.source.file_name()) else {
        return Outcome::Failed("not a file path".to_string());
    };
    let walked = worker.source_parent.get(parent, || {
        atomic_fs::walk_nofollow(&anchors.root, &components(parent))
    });
    let source_dir = match walked {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Outcome::Missing,
        Err(e) if is_not_a_directory(&e) => return Outcome::Skipped("not a regular file"),
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let source = match atomic_fs::open_source(source_dir, name) {
        Ok(Source::Regular(file)) => file,
        Ok(Source::Missing) => return Outcome::Missing,
        Ok(Source::NotRegular) => return Outcome::Skipped("not a regular file"),
        Ok(Source::Changed) => return Outcome::Skipped("changed while being opened"),
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    if let Some(check) = safe {
        match holds_live(check.filters, check.size_mode, &live(source.stat())) {
            Verdict::Holds => {}
            Verdict::NotRegular => return Outcome::Skipped("not a regular file"),
            Verdict::Changed(reason) => return Outcome::Skipped(reason),
        }
    }
    let rel_parent = item.rel.parent().unwrap_or(Path::new(""));
    let walked = worker.dest_parent.get(rel_parent, || {
        atomic_fs::walk_create_nofollow(&anchors.into, &components(rel_parent))
    });
    let dest_dir = match walked {
        Ok(dir) => dir,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    match copy_atomic(&source, dest_dir, name) {
        Ok(Placed::Done { owner_kept }) => Outcome::Done { owner_kept },
        Ok(Placed::Existing) => Outcome::Existing,
        Ok(Placed::SourceChanged) => Outcome::Skipped("changed during the copy"),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// What `xdu-mv` does with a file whose destination is on another filesystem, where no rename
/// can reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossFs {
    /// Fail the file, naming the cause, and leave the source where it is.
    Refuse,
    /// Copy the file there durably, check that the source did not change, then remove the
    /// source (`atomic_fs::move_across`).
    Copy,
}

/// The cross-filesystem policy this build ships.
pub const CROSS_FS: CrossFs = CrossFs::Copy;

/// The failure reported for a file the `Refuse` policy kept at its source.
pub const CROSSING_REFUSED: &str = "crossing filesystems is not supported by this build";

/// Move one planned file by a no-replace rename: walk to its source directory without
/// following a symlink, and let `move_same_fs` judge the object at the name (only a regular
/// file moves, with or without `--safe`), re-check it under `--safe` against that same
/// `lstat`, create its destination directory, and rename it. A rename the kernel answers
/// with `EXDEV` goes where `cross` says: under `Copy`, to `move_across`, which re-checks
/// `--safe` against the `fstat` of the descriptor it copies from.
fn move_one(
    anchors: &Anchors,
    worker: &mut Worker,
    item: &Planned,
    safe: Option<SafeCheck<'_>>,
    cross: CrossFs,
    faults: Faults,
) -> Outcome {
    let (Some(parent), Some(name)) = (item.source.parent(), item.source.file_name()) else {
        return Outcome::Failed("not a file path".to_string());
    };
    let walked = worker.source_parent.get(parent, || {
        atomic_fs::walk_nofollow(&anchors.root, &components(parent))
    });
    let source_dir = match walked {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Outcome::Missing,
        Err(e) if is_not_a_directory(&e) => return Outcome::Skipped("not a regular file"),
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let rel_parent = item.rel.parent().unwrap_or(Path::new(""));
    let dest_cache = &mut worker.dest_parent;
    let dest = move || {
        let cache = dest_cache;
        cache.get(rel_parent, || {
            atomic_fs::walk_create_nofollow(&anchors.into, &components(rel_parent))
        })
    };
    let admit = |stat: &Stat| match safe {
        None => Ok(()),
        Some(check) => match holds_live(check.filters, check.size_mode, &live(stat)) {
            Verdict::Holds => Ok(()),
            Verdict::NotRegular => Err("not a regular file"),
            Verdict::Changed(reason) => Err(reason),
        },
    };
    match atomic_fs::move_same_fs_with(source_dir, name, dest, name, admit, faults, &mut NoHook) {
        Ok(Moved::Done) => Outcome::Done { owner_kept: true },
        Ok(Moved::Missing) => Outcome::Missing,
        Ok(Moved::NotRegular) => Outcome::Skipped("not a regular file"),
        Ok(Moved::Rejected(reason)) => Outcome::Skipped(reason),
        Ok(Moved::Existing) => Outcome::Existing,
        Ok(Moved::CrossDevice) => match cross {
            CrossFs::Refuse => Outcome::Failed(CROSSING_REFUSED.to_string()),
            CrossFs::Copy => {
                let walked = worker.dest_parent.get(rel_parent, || {
                    atomic_fs::walk_create_nofollow(&anchors.into, &components(rel_parent))
                });
                let dest_dir = match walked {
                    Ok(dir) => dir,
                    Err(e) => return Outcome::Failed(e.to_string()),
                };
                let crossed = atomic_fs::move_across_with(
                    source_dir,
                    name,
                    dest_dir,
                    name,
                    admit,
                    faults,
                    &mut NoHook,
                );
                match crossed {
                    Ok(Crossed::Done { owner_kept }) => Outcome::Done { owner_kept },
                    Ok(Crossed::Missing) => Outcome::Missing,
                    Ok(Crossed::NotRegular) => Outcome::Skipped("not a regular file"),
                    Ok(Crossed::Swapped) => Outcome::Skipped("changed while being opened"),
                    Ok(Crossed::Rejected(reason)) => Outcome::Skipped(reason),
                    Ok(Crossed::Existing) => Outcome::Existing,
                    Ok(Crossed::SourceChanged) => Outcome::Skipped("changed during the copy"),
                    Err(Stop::Err(e)) => Outcome::Failed(e.to_string()),
                    Err(Stop::Abandoned) => Outcome::Failed("move abandoned".to_string()),
                }
            }
        },
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// The words one tool uses for its action.
#[derive(Clone, Copy, Debug)]
struct Verbs {
    tool: &'static str,
    /// The prompt's verb: `Copy`.
    ask: &'static str,
    /// The refusal's verb: `copy`.
    act: &'static str,
    /// The listing's and dry run's participle: `copied`.
    done: &'static str,
    /// The summary's count label: `Copied`.
    count: &'static str,
}

const CP: Verbs = Verbs {
    tool: "xdu-cp",
    ask: "Copy",
    act: "copy",
    done: "copied",
    count: "Copied",
};

const MV: Verbs = Verbs {
    tool: "xdu-mv",
    ask: "Move",
    act: "move",
    done: "moved",
    count: "Moved",
};

/// What a run does with each planned file.
#[derive(Clone, Copy, Debug)]
enum Action {
    Copy,
    Move(CrossFs),
}

impl Action {
    fn verbs(self) -> Verbs {
        match self {
            Action::Copy => CP,
            Action::Move(_) => MV,
        }
    }
}

/// Act on every planned file on a local pool of `jobs` threads, printing and counting each
/// outcome on the calling thread in path order.
///
/// The pool is local, never rayon's global one: the crawler owns the global pool's shape, and
/// a library caller (or a test) running two selections must not share or fight over it.
fn execute(
    verbs: Verbs,
    map: &RootMap,
    planned: &[Planned],
    jobs: usize,
    verbose: bool,
    act: impl Fn(&Anchors, &mut Worker, &Planned) -> Outcome + Sync,
) -> Result<Summary> {
    let anchors = Anchors {
        root: DirHandle::open(Path::new("/")).context("cannot open /")?,
        into: DirHandle::open(map.into_dir())
            .with_context(|| format!("cannot open --into {}", map.into_dir().display()))?,
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .context("cannot start the worker pool")?;
    let mut summary = Summary::default();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for chunk in planned.chunks(CHUNK) {
        let outcomes: Vec<Outcome> = pool.install(|| {
            chunk
                .par_iter()
                .map_init(Worker::default, |worker, item| act(&anchors, worker, item))
                .collect()
        });
        for (item, outcome) in chunk.iter().zip(&outcomes) {
            summary.add(outcome);
            let source = item.source.display();
            let dest = map.destination(&item.rel);
            match outcome {
                Outcome::Failed(message) => {
                    eprintln!("{}: {source}: {message}", verbs.tool);
                }
                _ if !verbose => {}
                Outcome::Done { owner_kept: true } => {
                    writeln!(out, "{}: {source} -> {}", verbs.done, dest.display())?
                }
                Outcome::Done { owner_kept: false } => writeln!(
                    out,
                    "{} (ownership not preserved): {source} -> {}",
                    verbs.done,
                    dest.display()
                )?,
                Outcome::Missing => writeln!(out, "missing: {source}")?,
                Outcome::Existing => writeln!(out, "existing: {}", dest.display())?,
                Outcome::Skipped(reason) => writeln!(out, "skipped ({reason}): {source}")?,
            }
        }
    }
    out.flush()?;
    Ok(summary)
}

/// `--into` must exist and be a directory; the operator's own symlink is followed once and the
/// result canonicalized.
fn checked_into(into: &Path) -> Result<PathBuf> {
    let meta = fs::metadata(into)
        .map_err(|e| anyhow!("--into {} does not exist ({e})", into.display()))?;
    if !meta.is_dir() {
        bail!("--into {} is not a directory", into.display());
    }
    into.canonicalize()
        .with_context(|| format!("cannot resolve --into {}", into.display()))
}

/// `xdu-cp`: copy the selected set under `--into`. Returns the process exit status.
pub fn run_cp(args: &XduCpArgs) -> Result<i32> {
    run(Action::Copy, &args.select, &args.into, &args.root)
}

/// `xdu-mv`: move the selected set under `--into`. Returns the process exit status.
pub fn run_mv(args: &XduMvArgs) -> Result<i32> {
    run(Action::Move(CROSS_FS), &args.select, &args.into, &args.root)
}

/// One select-then-act run.
///
/// The order is the refusal order: `--into`, `--root`, the index gates, the empty set, the
/// whole-set mapping, then the preview or the prompt, and only then a write.
fn run(action: Action, select_args: &SelectArgs, into: &Path, root: &Path) -> Result<i32> {
    let verbs = action.verbs();
    let into = checked_into(into)?;
    let root = root
        .canonicalize()
        .with_context(|| format!("--root {} does not exist", root.display()))?;
    let selection = select(select_args)?;
    if let Some(warning) = &selection.warning {
        eprintln!("{warning}");
    }
    if selection.rows.is_empty() {
        println!("No matching files found.");
        return Ok(0);
    }
    let map = RootMap::new(root, into);
    let planned = check_whole_set(&map, &selection.rows)
        .map_err(|refusal| anyhow!("refusing to {} anything: {refusal}", verbs.act))?;

    if select_args.dry_run {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for item in &planned {
            writeln!(
                out,
                "{} -> {}",
                item.source.display(),
                map.destination(&item.rel).display()
            )?;
        }
        writeln!(
            out,
            "\n{} file(s) would be {} into {}",
            planned.len(),
            verbs.done,
            map.into_dir().display()
        )?;
        return Ok(0);
    }

    if !select_args.force {
        let question = format!(
            "{} {} file(s) into {}?",
            verbs.ask,
            planned.len(),
            map.into_dir().display()
        );
        let proceed = confirm(&question, &mut io::stdin().lock(), &mut io::stderr())?;
        if !proceed {
            eprintln!("Aborted.");
            return Ok(0);
        }
    }

    let safe = select_args.safe.then_some(SafeCheck {
        filters: &selection.filters,
        size_mode: selection.size_mode,
    });
    let jobs = select_args.jobs;
    let verbose = select_args.verbose;
    let summary = match action {
        Action::Copy => execute(
            verbs,
            &map,
            &planned,
            jobs,
            verbose,
            |anchors, worker, item| copy_one(anchors, worker, item, safe),
        )?,
        Action::Move(cross) => execute(
            verbs,
            &map,
            &planned,
            jobs,
            verbose,
            |anchors, worker, item| move_one(anchors, worker, item, safe, cross, Faults::default()),
        )?,
    };
    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(out)?;
    for line in summary.lines(verbs.count) {
        writeln!(out, "{line}")?;
    }
    out.flush()?;
    Ok(summary.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(root: &str, into: &str) -> RootMap {
        RootMap::new(PathBuf::from(root), PathBuf::from(into))
    }

    fn row(path: &str) -> Row {
        Row {
            path: path.to_string(),
            size: 0,
            uid: 0,
            gid: 0,
            mode: 0,
            atime: 0,
            mtime: 0,
        }
    }

    #[test]
    fn a_row_outside_root_or_with_a_dot_dot_component_is_refused() {
        let m = map("/data", "/cold");
        assert_eq!(m.map("/data/a/b.txt").unwrap(), PathBuf::from("a/b.txt"));
        assert_eq!(
            m.destination(Path::new("a/b.txt")),
            PathBuf::from("/cold/a/b.txt")
        );

        for outside in ["/data2/f", "/dat/f", "/other/data/f", "/data"] {
            let refusal = m.map(outside).unwrap_err();
            assert_eq!(refusal.path, outside);
            assert!(
                refusal.reason.contains("not under --root /data"),
                "{refusal}"
            );
        }
        for climbing in [
            "/data/../etc/passwd",
            "/data/a/../../etc/passwd",
            "/data/./a",
            "/data//a",
            "/data/a/",
            "data/a",
            "",
        ] {
            let refusal = m.map(climbing).unwrap_err();
            assert!(
                refusal.reason.contains("normalized"),
                "{climbing}: {refusal}"
            );
        }

        let whole = map("/", "/cold");
        assert_eq!(whole.map("/data/f").unwrap(), PathBuf::from("data/f"));
        let onto_itself = map("/data", "/data");
        assert!(
            onto_itself
                .map("/data/f")
                .unwrap_err()
                .reason
                .contains("equal to its own")
        );

        let rows = [
            row("/data/a"),
            row("/data/b"),
            row("/elsewhere/c"),
            row("/zz/d"),
        ];
        let first = check_whole_set(&m, &rows).unwrap_err();
        assert_eq!(first.path, "/elsewhere/c");
        let planned = check_whole_set(&m, &rows[..2]).unwrap();
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[1].rel, PathBuf::from("b"));
    }

    /// A rename answered with `EXDEV` under the `Refuse` policy fails the file with the cause
    /// and leaves the source, inode and bytes, where it was; nothing appears at the
    /// destination.
    #[test]
    fn a_cross_device_move_under_the_refuse_policy_keeps_the_source() {
        use std::os::unix::fs::MetadataExt;

        let t = tempfile::TempDir::new().unwrap();
        let base = t.path().canonicalize().unwrap();
        let source = base.join("src/u/f.dat");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source)
            .unwrap();
        file.write_all(b"STAYS").unwrap();
        drop(file);
        let inode = fs::metadata(&source).unwrap().ino();
        let into = base.join("into");
        fs::create_dir(&into).unwrap();

        let anchors = Anchors {
            root: DirHandle::open(Path::new("/")).unwrap(),
            into: DirHandle::open(&into).unwrap(),
        };
        let item = Planned {
            source: source.clone(),
            rel: PathBuf::from("u/f.dat"),
        };
        let exdev = Faults {
            force_cross_device: true,
            ..Faults::default()
        };
        for link_route in [false, true] {
            let faults = Faults {
                force_link_route: link_route,
                ..exdev
            };
            let got = move_one(
                &anchors,
                &mut Worker::default(),
                &item,
                None,
                CrossFs::Refuse,
                faults,
            );
            assert_eq!(
                got,
                Outcome::Failed(CROSSING_REFUSED.to_string()),
                "link route: {link_route}"
            );
            assert_eq!(fs::read(&source).unwrap(), b"STAYS");
            assert_eq!(fs::metadata(&source).unwrap().ino(), inode);
            assert!(fs::symlink_metadata(into.join("u/f.dat")).is_err());
        }

        // The same file, the same policy, and no fault: it moves.
        let got = move_one(
            &anchors,
            &mut Worker::default(),
            &item,
            None,
            CrossFs::Refuse,
            Faults::default(),
        );
        assert_eq!(got, Outcome::Done { owner_kept: true });
        assert_eq!(fs::metadata(into.join("u/f.dat")).unwrap().ino(), inode);
        assert!(fs::symlink_metadata(&source).is_err());
    }

    /// The policy the binary ships takes a rename answered with `EXDEV` to the copy path: the
    /// file lands as a new inode with its bytes, and the source is gone.
    #[test]
    fn the_production_policy_routes_exdev_to_the_copy_path() {
        use std::os::unix::fs::MetadataExt;

        assert_eq!(CROSS_FS, CrossFs::Copy);
        for link_route in [false, true] {
            let t = tempfile::TempDir::new().unwrap();
            let base = t.path().canonicalize().unwrap();
            let source = base.join("src/u/f.dat");
            fs::create_dir_all(source.parent().unwrap()).unwrap();
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&source)
                .unwrap();
            file.write_all(b"TRAVELS").unwrap();
            drop(file);
            let inode = fs::metadata(&source).unwrap().ino();
            let into = base.join("into");
            fs::create_dir(&into).unwrap();

            let anchors = Anchors {
                root: DirHandle::open(Path::new("/")).unwrap(),
                into: DirHandle::open(&into).unwrap(),
            };
            let item = Planned {
                source: source.clone(),
                rel: PathBuf::from("u/f.dat"),
            };
            let faults = Faults {
                force_cross_device: true,
                force_link_route: link_route,
                ..Faults::default()
            };
            let got = move_one(
                &anchors,
                &mut Worker::default(),
                &item,
                None,
                CROSS_FS,
                faults,
            );
            assert!(
                matches!(got, Outcome::Done { .. }),
                "link route: {link_route}: {got:?}"
            );
            let landed = into.join("u/f.dat");
            assert_eq!(fs::read(&landed).unwrap(), b"TRAVELS");
            assert_ne!(
                fs::metadata(&landed).unwrap().ino(),
                inode,
                "renamed, not copied"
            );
            assert!(fs::symlink_metadata(&source).is_err(), "the source stayed");
            let leftovers = fs::read_dir(into.join("u")).unwrap().count();
            assert_eq!(leftovers, 1, "a temp was left beside the copy");
        }
    }

    #[test]
    fn only_y_or_yes_confirms() {
        for (answer, expected) in [
            ("y\n", true),
            ("YES\n", true),
            (" yes \n", true),
            ("n\n", false),
            ("\n", false),
            ("", false),
            ("yep\n", false),
        ] {
            let mut err = Vec::new();
            let got = confirm("Copy 2 file(s) into /x?", &mut answer.as_bytes(), &mut err).unwrap();
            assert_eq!(got, expected, "{answer:?}");
            assert_eq!(
                String::from_utf8(err).unwrap(),
                "Copy 2 file(s) into /x? [y/N] "
            );
        }
    }

    #[test]
    fn the_summary_counts_each_outcome_and_sets_the_exit_status() {
        let mut s = Summary::default();
        for o in [
            Outcome::Done { owner_kept: true },
            Outcome::Done { owner_kept: false },
            Outcome::Missing,
            Outcome::Skipped("not a regular file"),
        ] {
            s.add(&o);
        }
        assert_eq!(
            s.lines("Copied"),
            [
                "Copied: 2",
                "Missing: 1",
                "Skipped: 1",
                "Ownership not preserved: 1"
            ]
        );
        assert_eq!(s.exit_code(), 0);
        s.add(&Outcome::Existing);
        assert_eq!(s.exit_code(), 1);
        let mut f = Summary::default();
        f.add(&Outcome::Failed("x".into()));
        assert_eq!(f.lines("Copied"), ["Copied: 0", "Failed: 1"]);
        assert_eq!(f.exit_code(), 1);
    }
}
