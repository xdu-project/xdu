//! Integration tests for the `xdu-view` listing statements over a local index.
//!
//! `xdu-view` itself is an interactive terminal program; the statements it sends are the
//! `xdu::view_query` functions these tests call, against indexes the real `xdu` binary builds.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use duckdb::Connection;
use tempfile::TempDir;

use common::{
    VIEW_TREE, build_index, create_test_file, set_atime_at, view_tree, view_tree_children,
    walk_view,
};
use xdu::view_query::{self, DirEntry};
use xdu::{IndexLocator, QueryFilters, SortMode};

/// Both views list every child under its full name below a non-ASCII crawled root, a
/// non-ASCII partition and non-ASCII directories at every depth, every listed directory can be
/// entered, and each listing reports what the same tree under ASCII names reports.
#[test]
fn non_ascii_directories_list_full_names_in_both_views() {
    let work = TempDir::new().unwrap();
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let partitions = [("données", "donnees"), ("p", "p")];

    let source = work.path().join("hôme-ü/src");
    let twin_source = work.path().join("home-u/src");
    let mut files = Vec::new();
    for (partition, twin) in partitions {
        files.extend(view_tree(&source, partition, false, epoch));
        view_tree(&twin_source, twin, true, epoch);
    }
    let index = work.path().join("index");
    let twin_index = work.path().join("twin-index");
    build_index(&source, &index);
    build_index(&twin_source, &twin_index);

    // The crawler stores canonical paths; above partition p, only the crawled root is non-ASCII.
    let root = source.canonicalize().unwrap().to_str().unwrap().to_string();
    let twin_root = twin_source
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(!root.is_ascii(), "{root}");
    assert!(twin_root.is_ascii(), "{twin_root}");
    let expected = view_tree_children(&root, &files);
    let want_files: BTreeMap<String, i64> = files
        .iter()
        .map(|(rel, size)| (format!("{root}/{rel}"), *size as i64))
        .collect();

    // Each name's ASCII twin, from the fixture's own pairs.
    let mut twin_names: HashMap<String, String> = partitions
        .iter()
        .map(|(name, twin)| (name.to_string(), twin.to_string()))
        .collect();
    for (name, twin, _, _) in VIEW_TREE {
        for (n, t) in name.split('/').zip(twin.split('/')) {
            twin_names.insert(n.to_string(), t.to_string());
        }
    }
    let twin_of = |name: &str| -> String {
        twin_names
            .get(name)
            .unwrap_or_else(|| panic!("no twin for {name}"))
            .clone()
    };

    let conn = xdu::engine::open_connection().unwrap();
    let local = IndexLocator::Local(index.canonicalize().unwrap());
    let twin = IndexLocator::Local(twin_index.canonicalize().unwrap());
    let mut listed = 0;
    for view in ["list", "tree"] {
        for (partition, twin_partition) in partitions {
            let walk = walk_view(&conn, &local, partition, "local", view, &expected);
            let reached: BTreeMap<String, i64> = walk
                .files
                .iter()
                .filter(|(path, _)| path.starts_with(&format!("{root}/{partition}/")))
                .map(|(path, size)| (path.clone(), *size))
                .collect();
            let want: BTreeMap<String, i64> = want_files
                .iter()
                .filter(|(path, _)| path.starts_with(&format!("{root}/{partition}/")))
                .map(|(path, size)| (path.clone(), *size))
                .collect();
            assert_eq!(
                reached.len(),
                VIEW_TREE.len(),
                "local {view} under {partition}"
            );
            assert_eq!(reached, walk.files, "local {view} left {partition}");
            assert_eq!(
                reached, want,
                "local {view} reached every file of {partition}"
            );

            for (dir, entries) in &walk.listings {
                let rel = dir.strip_prefix(&format!("{root}/")).unwrap();
                let twin_dir = format!(
                    "{twin_root}/{}",
                    rel.split('/').map(twin_of).collect::<Vec<_>>().join("/")
                );
                let filters = QueryFilters::new();
                let twin_entries = match view {
                    "list" => view_query::list_directory(
                        &conn,
                        &twin,
                        twin_partition,
                        &twin_dir,
                        &filters,
                        SortMode::Name,
                    ),
                    _ => view_query::tree_directory(
                        &conn,
                        &twin,
                        twin_partition,
                        &twin_dir,
                        &filters,
                        SortMode::Name,
                    ),
                }
                .unwrap();
                let mapped: BTreeMap<String, (bool, i64, i64, i64)> = entries
                    .iter()
                    .map(|e| {
                        let fields = (e.is_dir, e.total_size, e.file_count, e.latest_atime);
                        (twin_of(&e.name), fields)
                    })
                    .collect();
                let twins: BTreeMap<String, (bool, i64, i64, i64)> = twin_entries
                    .iter()
                    .map(|e| {
                        let fields = (e.is_dir, e.total_size, e.file_count, e.latest_atime);
                        (e.name.clone(), fields)
                    })
                    .collect();
                assert_eq!(mapped.len(), entries.len(), "local {view}: a name repeats");
                assert!(!twins.is_empty(), "the twin of {dir} lists nothing");
                assert_eq!(mapped, twins, "local {view} of {dir} against {twin_dir}");
                listed += entries.len();
            }
        }
    }
    // Per view and partition: the root's 6 entries, alpha's 2, and one under each of beta,
    // the accented directory, its `a`, the lossy one and clef.
    assert_eq!(listed, 2 * 2 * (6 + 2 + 1 + 1 + 1 + 1 + 1));
}

// A key=value directory name reads like any other.
//
// DuckDB's Hive detection turns a `key=value` directory name that every globbed path carries
// into a column `key` that shadows the row's own. Each case compares an index whose partition
// or index directory has such a name against a bare twin built from the same files, through
// every statement xdu-view sends. The fixtures stay in this file rather than
// `tests/common/mod.rs`, which these tests must not change: they have to build on a release
// base that lacks that file's later helpers.

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
/// views group by, and a key naming no column.
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

/// The atime `write_tree` gives the i-th file.
fn atime_of(epoch: u64, i: usize) -> u64 {
    epoch - 60 * (i as u64 + 1)
}

/// Write `files` under `root`, each accessed at `atime_of` its position.
fn write_tree(root: &Path, files: &[(&str, usize)], epoch: u64) {
    for (i, (rel, size)) in files.iter().enumerate() {
        let file = root.join(rel);
        create_test_file(&file, *size).unwrap();
        set_atime_at(&file, atime_of(epoch, i)).unwrap();
    }
}

/// Index `tree` into `index`, making the directories above it first, and locate it.
fn build_index_at(tree: &Path, index: &Path) -> IndexLocator {
    std::fs::create_dir_all(index.parent().unwrap()).unwrap();
    build_index(tree, index);
    IndexLocator::Local(index.canonicalize().unwrap())
}

/// The filter sets every view call is compared under.
fn key_value_filter_sets() -> Vec<(&'static str, QueryFilters)> {
    vec![
        ("no filter", QueryFilters::new()),
        (
            "*.txt",
            QueryFilters::new()
                .with_path_pattern(Some("*.txt".to_string()), false)
                .unwrap(),
        ),
        (
            "max 1K",
            QueryFilters::new().with_max_size(Some("1K")).unwrap(),
        ),
        (
            "min 1M",
            QueryFilters::new().with_min_size(Some("1M")).unwrap(),
        ),
        (
            "older than 3650",
            QueryFilters::new().with_older_than(Some(3650)),
        ),
    ]
}

/// `got` against `want`: the same entries with the same fields and, under `SortMode::Name`,
/// the only order without ties, in the same order.
fn assert_same_entries(got: &[DirEntry], want: &[DirEntry], sort_mode: SortMode, what: &str) {
    let by_name = |entries: &[DirEntry]| {
        let mut entries = entries.to_vec();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    };
    if sort_mode == SortMode::Name {
        assert_eq!(got, want, "{what}");
    } else {
        assert_eq!(by_name(got), by_name(want), "{what}");
    }
}

/// Every statement xdu-view sends, against `index` and against its bare twin, compared call by
/// call: `partitions` under every sort, `partition_root`, both views of every directory from
/// each partition's root down, and `root_files`, each under every filter set. `partitions`
/// pairs each partition's name in `index` with its name in the twin, and `to_index` maps a twin
/// name or path to the index's. Returns the number of calls compared.
fn compare_views(
    conn: &Connection,
    index: &IndexLocator,
    twin: &IndexLocator,
    partitions: &[(&str, &str)],
    dirs: &BTreeSet<String>,
    to_index: &dyn Fn(&str) -> String,
) -> usize {
    let mapped = |entries: Vec<DirEntry>| -> Vec<DirEntry> {
        entries
            .into_iter()
            .map(|e| DirEntry {
                name: to_index(&e.name),
                path: to_index(&e.path),
                ..e
            })
            .collect()
    };
    let view = |name: &str,
                locator: &IndexLocator,
                partition: &str,
                dir: &str,
                filters: &QueryFilters,
                sort_mode: SortMode| {
        match name {
            "list" => view_query::list_directory(conn, locator, partition, dir, filters, sort_mode),
            _ => view_query::tree_directory(conn, locator, partition, dir, filters, sort_mode),
        }
        .unwrap()
    };
    let mut compared = 0;
    let mut walked = BTreeSet::new();
    for (partition, twin_partition) in partitions {
        let root = view_query::partition_root(conn, index, partition).unwrap();
        let twin_root = view_query::partition_root(conn, twin, twin_partition).unwrap();
        assert_eq!(root, to_index(&twin_root), "partition_root of {partition}");
        compared += 1;
        let mut pending = vec![(root, twin_root)];
        while let Some((dir, twin_dir)) = pending.pop() {
            for (label, filters) in key_value_filter_sets() {
                for sort_mode in SortMode::ALL {
                    for name in ["list", "tree"] {
                        let got = view(name, index, partition, &dir, &filters, sort_mode);
                        let want = mapped(view(
                            name,
                            twin,
                            twin_partition,
                            &twin_dir,
                            &filters,
                            sort_mode,
                        ));
                        let what = format!("{name} of {dir} under {label}, sorted by {sort_mode}");
                        assert_same_entries(&got, &want, sort_mode, &what);
                        compared += 1;
                        if label == "no filter" && sort_mode == SortMode::Name && name == "list" {
                            for entry in got.iter().filter(|e| e.is_dir) {
                                let twin_child = format!("{twin_dir}/{}", entry.name);
                                pending.push((entry.path.clone(), twin_child));
                            }
                        }
                    }
                }
            }
            walked.insert(dir);
        }
    }
    assert_eq!(&walked, dirs, "the directories the list view reached");

    for (label, filters) in key_value_filter_sets() {
        for sort_mode in SortMode::ALL {
            let got = view_query::partitions(conn, index, &filters, sort_mode).unwrap();
            let want = mapped(view_query::partitions(conn, twin, &filters, sort_mode).unwrap());
            if label == "no filter" {
                assert_eq!(got.len(), partitions.len(), "partitions: {got:?}");
            }
            let what = format!("partitions under {label}, sorted by {sort_mode}");
            assert_same_entries(&got, &want, sort_mode, &what);
            compared += 1;
        }
        let got = view_query::root_files(conn, index, &filters).unwrap();
        let want = mapped(view_query::root_files(conn, twin, &filters).unwrap());
        // The root files carry no order of their own; the caller sorts the merged list.
        assert_same_entries(
            &got,
            &want,
            SortMode::SizeDesc,
            &format!("root_files under {label}"),
        );
        compared += 1;
    }
    compared
}

/// A partition named `size=0` reads as a name: its size, count and access time, and every
/// listing below it, are those its bare twin reports.
#[test]
fn view_reads_a_key_value_partition_as_a_bare_name() {
    let work = TempDir::new().unwrap();
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let source = work.path().join("source");
    write_tree(&source.join("size-0"), PARTITION_FILES, epoch);
    // The twin is indexed from the same files while the partition still has a bare name; the
    // rename keeps every inode, so the two indexes differ in that one name.
    let twin = build_index_at(&source, &work.path().join("twin"));
    std::fs::rename(source.join("size-0"), source.join("size=0")).unwrap();
    let index = build_index_at(&source, &work.path().join("index"));
    let root = source.canonicalize().unwrap().to_str().unwrap().to_string();
    let conn = xdu::engine::open_connection().unwrap();

    let total: i64 = PARTITION_FILES.iter().map(|(_, size)| *size as i64).sum();
    let latest = atime_of(epoch, 0) as i64;
    let got = view_query::partitions(&conn, &index, &QueryFilters::new(), SortMode::Name);
    assert!(
        matches!(&got, Ok(entries) if entries.len() == 1
            && entries[0].name == "size=0"
            && entries[0].total_size == total
            && entries[0].file_count == PARTITION_FILES.len() as i64
            && entries[0].latest_atime == latest),
        "key=value partition: partitions reported {got:?}, not size=0 with total_size {total}, \
         {} files and latest_atime {latest}",
        PARTITION_FILES.len()
    );

    let (bare_dir, kv_dir) = (format!("{root}/size-0"), format!("{root}/size=0"));
    let to_index = |s: &str| -> String {
        match s {
            "size-0" => "size=0".to_string(),
            _ => s.replace(&bare_dir, &kv_dir),
        }
    };
    let dirs = BTreeSet::from([kv_dir.clone(), format!("{kv_dir}/sub")]);
    let compared = compare_views(
        &conn,
        &index,
        &twin,
        &[("size=0", "size-0")],
        &dirs,
        &to_index,
    );
    // partition_root, two directories in both views, then partitions and root_files.
    let (filters, sorts) = (key_value_filter_sets().len(), SortMode::ALL.len());
    assert_eq!(
        compared,
        1 + 2 * filters * sorts * 2 + filters * (sorts + 1)
    );
}

/// An index stored under a `key=value` directory reads as one stored under a bare name: access
/// times are the files' own, and every view call returns what the bare index returns.
#[test]
fn view_reads_an_index_under_a_key_value_directory_as_a_bare_one() {
    let work = TempDir::new().unwrap();
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let tree = work.path().join("tree");
    write_tree(&tree, INDEX_DIR_FILES, epoch);
    let bare = build_index_at(&tree, &work.path().join("bare/index"));
    let keyed: Vec<IndexLocator> = KEYS
        .iter()
        .map(|key| build_index_at(&tree, &work.path().join(format!("{key}=0/index"))))
        .collect();
    let root = tree.canonicalize().unwrap().to_str().unwrap().to_string();
    let conn = xdu::engine::open_connection().unwrap();

    // Each entry directly under alpha and its latest atime, from the fixture.
    let mut want: BTreeMap<String, i64> = BTreeMap::new();
    for (i, (rel, _)) in INDEX_DIR_FILES.iter().enumerate() {
        if let Some(rest) = rel.strip_prefix("alpha/") {
            let name = rest.split('/').next().unwrap().to_string();
            let atime = want.entry(name).or_insert(0);
            *atime = (*atime).max(atime_of(epoch, i) as i64);
        }
    }
    let atime = IndexLocator::Local(work.path().join("atime=0/index").canonicalize().unwrap());
    let alpha = format!("{root}/alpha");
    let older = QueryFilters::new().with_older_than(Some(3650));
    let stale = view_query::list_directory(&conn, &atime, "alpha", &alpha, &older, SortMode::Name);
    let fresh = view_query::list_directory(
        &conn,
        &atime,
        "alpha",
        &alpha,
        &QueryFilters::new(),
        SortMode::Name,
    )
    .map(|entries| {
        entries
            .into_iter()
            .map(|e| (e.name, e.latest_atime))
            .collect::<BTreeMap<_, _>>()
    });
    assert!(
        matches!(&stale, Ok(entries) if entries.is_empty())
            && matches!(&fresh, Ok(atimes) if *atimes == want),
        "key=value index directory: under atime=0, older than 3650 days listed {stale:?} and \
         the latest atimes read {fresh:?}, not {want:?}"
    );

    let dirs = BTreeSet::from([
        alpha.clone(),
        format!("{alpha}/deep"),
        format!("{root}/beta"),
    ]);
    let partitions = [("alpha", "alpha"), ("beta", "beta")];
    let identity = |s: &str| s.to_string();
    let mut compared = 0;
    for (key, index) in KEYS.iter().zip(&keyed) {
        let calls = compare_views(&conn, index, &bare, &partitions, &dirs, &identity);
        assert!(calls > 0, "under {key}=0");
        compared += calls;
    }
    // Per key: two partition roots, three directories in both views, then partitions and
    // root_files.
    let (filters, sorts) = (key_value_filter_sets().len(), SortMode::ALL.len());
    let per_key = 2 + 3 * filters * sorts * 2 + filters * (sorts + 1);
    assert_eq!(compared, KEYS.len() * per_key);
}
