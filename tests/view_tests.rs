//! Integration tests for the `xdu-view` listing statements over a local index.
//!
//! `xdu-view` itself is an interactive terminal program; the statements it sends are the
//! `xdu::view_query` functions these tests call, against indexes the real `xdu` binary builds.

mod common;

use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::TempDir;

use common::{VIEW_TREE, build_index, view_tree, view_tree_children, walk_view};
use xdu::view_query;
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
