//! The index queries behind `xdu-view`'s list and tree views.
//!
//! Every statement takes its `FROM` from `parquet_source`, so a local index gets the text it
//! always had, with Hive detection turned off, and an S3 index gets `read_parquet(?, …)` with
//! its glob bound. The one other value a statement draws from the index, the directory prefix
//! a listing matches with `path LIKE`, is bound on the S3 arm as well. Each statement is built
//! by a pure function returning its text and the values it binds, so a unit test can pin the
//! local text without a connection.

use anyhow::Result;
use duckdb::{Connection, params_from_iter};

use crate::{IndexLocator, QueryFilters, ROOT_PARTITION, SortMode, parquet_source};

/// One row of a view: a partition, a directory, or a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub total_size: i64,
    pub file_count: i64,
    pub latest_atime: i64,
}

/// The statement listing the files of the `__root__` partition, with its bound values.
pub fn root_files_sql(index: &IndexLocator, filters: &QueryFilters) -> (String, Vec<String>) {
    let (source, params) = parquet_source(index, Some(ROOT_PARTITION), false);
    let where_clause = filters.to_full_where_clause();

    let sql = format!(
        r#"
            SELECT path, size, atime
            FROM {source}
            {where_clause}
            "#,
        source = source,
        where_clause = where_clause
    );
    (sql, params)
}

/// Individual files from the `__root__` partition, each with `is_dir` false and a count of 1.
///
/// An index without a `__root__` partition has no files to match, which DuckDB reports as an
/// error; that, like any failure here, yields no entries.
pub fn root_files(
    conn: &Connection,
    index: &IndexLocator,
    filters: &QueryFilters,
) -> Result<Vec<DirEntry>> {
    let (sql, params) = root_files_sql(index, filters);

    let mut entries = Vec::new();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Ok(entries), // No __root__ partition exists
    };
    let mut rows = match stmt.query(params_from_iter(&params)) {
        Ok(r) => r,
        Err(_) => return Ok(entries),
    };

    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let size: i64 = row.get(1)?;
        let atime: i64 = row.get(2)?;
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        entries.push(DirEntry {
            name,
            path,
            is_dir: false,
            total_size: size,
            file_count: 1,
            latest_atime: atime,
        });
    }

    Ok(entries)
}

/// The statement listing every partition but `__root__`, with its bound values.
///
/// The partition name is the directory holding the chunk, recovered from the `filename`
/// pseudo-column. Not Hive partitioning: the layout uses bare directory names, and
/// `parquet_source` turns Hive detection off, so a partition or index directory named
/// `key=value` reads as a name and adds no column. When filters are active, only matching
/// files are counted and summed, and a partition with none is left out.
pub fn partitions_sql(
    index: &IndexLocator,
    filters: &QueryFilters,
    sort_mode: SortMode,
) -> (String, Vec<String>) {
    let (source, params) = parquet_source(index, None, true);

    // Exclude __root__ — its files are merged as individual entries
    let filter_clause = filters.to_where_clause();
    let having_clause = if filter_clause.is_empty() {
        format!(
            "HAVING partition IS NOT NULL AND partition != '' AND partition != '{}'",
            ROOT_PARTITION
        )
    } else {
        format!(
            "HAVING partition IS NOT NULL AND partition != '' AND partition != '{}' AND SUM(CASE WHEN {} THEN 1 ELSE 0 END) > 0",
            ROOT_PARTITION, filter_clause
        )
    };

    let sql = if filters.is_active() {
        format!(
            r#"
                SELECT 
                    regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1) as partition,
                    SUM(CASE WHEN {filter} THEN size ELSE 0 END) as total_size,
                    MAX(CASE WHEN {filter} THEN atime ELSE 0 END) as latest_atime,
                    SUM(CASE WHEN {filter} THEN 1 ELSE 0 END) as file_count
                FROM {source}
                GROUP BY partition
                {having}
                ORDER BY {order}
                "#,
            filter = filter_clause,
            source = source,
            having = having_clause,
            order = sort_mode.to_partition_order_by()
        )
    } else {
        format!(
            r#"
                SELECT 
                    regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1) as partition,
                    SUM(size) as total_size,
                    MAX(atime) as latest_atime,
                    COUNT(*) as file_count
                FROM {source}
                GROUP BY partition
                HAVING partition IS NOT NULL AND partition != '' AND partition != '{root}'
                ORDER BY {order}
                "#,
            source = source,
            root = ROOT_PARTITION,
            order = sort_mode.to_partition_order_by()
        )
    };
    (sql, params)
}

/// Every partition but `__root__`, as directory entries in `sort_mode`'s order.
///
/// The caller merges in `root_files` and sorts the merged list itself.
pub fn partitions(
    conn: &Connection,
    index: &IndexLocator,
    filters: &QueryFilters,
    sort_mode: SortMode,
) -> Result<Vec<DirEntry>> {
    let (sql, params) = partitions_sql(index, filters, sort_mode);

    let mut entries = Vec::new();
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(&params))?;

    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let total_size: i64 = row.get(1)?;
        let latest_atime: i64 = row.get(2)?;
        let file_count: i64 = row.get(3)?;
        entries.push(DirEntry {
            name: name.clone(),
            path: name,
            is_dir: true,
            total_size,
            file_count,
            latest_atime,
        });
    }

    Ok(entries)
}

/// The statement finding a partition's shortest path, with its bound values.
pub fn partition_root_sql(index: &IndexLocator, partition: &str) -> (String, Vec<String>) {
    let (source, params) = parquet_source(index, Some(partition), false);

    // Get the shortest path to find the common root
    let sql = format!("SELECT path FROM {} ORDER BY length(path) LIMIT 1", source);
    (sql, params)
}

/// The directory holding a partition's shortest path, or an empty string for an empty
/// partition.
pub fn partition_root(conn: &Connection, index: &IndexLocator, partition: &str) -> Result<String> {
    let (sql, params) = partition_root_sql(index, partition);

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(&params))?;

    if let Some(row) = rows.next()? {
        let sample_path: String = row.get(0)?;
        // Find the directory containing this file
        if let Some(pos) = sample_path.rfind('/') {
            return Ok(sample_path[..pos].to_string());
        }
    }

    Ok(String::new())
}

/// What a directory listing's statement is built from.
struct DirectoryQuery {
    source: String,
    params: Vec<String>,
    file_filter: String,
    /// The directory prefix's length in Unicode scalar values, the unit DuckDB's `substr` and
    /// `position` count, so the cut lands exactly after the prefix the `LIKE` matched.
    prefix_len: usize,
    order_by: &'static str,
}

impl DirectoryQuery {
    /// The parts of the statement listing the entries directly under `dir` in `partition`.
    ///
    /// A local index writes the directory prefix into a `LIKE` literal, as it always has; an S3
    /// index binds it, after the glob.
    fn new(
        index: &IndexLocator,
        partition: &str,
        dir: &str,
        filters: &QueryFilters,
        sort_mode: SortMode,
    ) -> DirectoryQuery {
        let (source, mut params) = parquet_source(index, Some(partition), false);

        // Build the path prefix we're looking at (with trailing slash for LIKE)
        let prefix = format!("{}/", dir);

        // Unicode scalar values, the unit DuckDB's substr and position count (not bytes).
        let prefix_len = prefix.chars().count();

        let prefix_match = match index {
            IndexLocator::Local(_) => format!("path LIKE '{}%'", prefix),
            IndexLocator::S3(_) => {
                params.push(format!("{}%", prefix));
                "path LIKE ?".to_string()
            }
        };

        // Build filter conditions
        let filter_clause = filters.to_where_clause();
        let file_filter = if filter_clause.is_empty() {
            prefix_match
        } else {
            format!("{} AND {}", prefix_match, filter_clause)
        };

        let order_by = sort_mode.to_order_by(sort_mode == SortMode::Name);

        DirectoryQuery {
            source,
            params,
            file_filter,
            prefix_len,
            order_by,
        }
    }
}

/// The list view's statement for the entries directly under `dir`, with its bound values.
///
/// It is the tree view's statement in a different layout; each view keeps the text it has
/// always sent.
pub fn list_directory_sql(
    index: &IndexLocator,
    partition: &str,
    dir: &str,
    filters: &QueryFilters,
    sort_mode: SortMode,
) -> (String, Vec<String>) {
    let query = DirectoryQuery::new(index, partition, dir, filters, sort_mode);
    let sql = format!(
        r#"
            WITH files AS (
                SELECT 
                    path,
                    size,
                    atime
                FROM {source}
                WHERE {file_filter}
            ),
            components AS (
                SELECT 
                    path,
                    size,
                    atime,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 
                        THEN substr(path, {prefix_len} + 1, position('/' IN substr(path, {prefix_len} + 1)) - 1)
                        ELSE substr(path, {prefix_len} + 1)
                    END as component,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 THEN true
                        ELSE false
                    END as is_dir
                FROM files
            )
            SELECT 
                component,
                bool_or(is_dir) as is_dir,
                SUM(size) as total_size,
                COUNT(*) as file_count,
                MAX(atime) as latest_atime
            FROM components
            WHERE component != '' AND component IS NOT NULL
            GROUP BY component
            ORDER BY {order_by}
            "#,
        source = query.source,
        file_filter = query.file_filter,
        prefix_len = query.prefix_len,
        order_by = query.order_by
    );
    (sql, query.params)
}

/// The tree view's statement for the entries directly under `dir`, with its bound values.
pub fn tree_directory_sql(
    index: &IndexLocator,
    partition: &str,
    dir: &str,
    filters: &QueryFilters,
    sort_mode: SortMode,
) -> (String, Vec<String>) {
    let query = DirectoryQuery::new(index, partition, dir, filters, sort_mode);
    let sql = format!(
        r#"
            WITH files AS (
                SELECT path, size, atime
                FROM {source}
                WHERE {file_filter}
            ),
            components AS (
                SELECT 
                    path, size, atime,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 
                        THEN substr(path, {prefix_len} + 1, position('/' IN substr(path, {prefix_len} + 1)) - 1)
                        ELSE substr(path, {prefix_len} + 1)
                    END as component,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 THEN true
                        ELSE false
                    END as is_dir
                FROM files
            )
            SELECT 
                component,
                bool_or(is_dir) as is_dir,
                SUM(size) as total_size,
                COUNT(*) as file_count,
                MAX(atime) as latest_atime
            FROM components
            WHERE component != '' AND component IS NOT NULL
            GROUP BY component
            ORDER BY {order_by}
            "#,
        source = query.source,
        file_filter = query.file_filter,
        prefix_len = query.prefix_len,
        order_by = query.order_by
    );
    (sql, query.params)
}

/// The entries directly under `dir` in `partition`, for the list view.
pub fn list_directory(
    conn: &Connection,
    index: &IndexLocator,
    partition: &str,
    dir: &str,
    filters: &QueryFilters,
    sort_mode: SortMode,
) -> Result<Vec<DirEntry>> {
    let (sql, params) = list_directory_sql(index, partition, dir, filters, sort_mode);
    directory_entries(conn, &sql, &params, dir)
}

/// The entries directly under `dir` in `partition`, for the tree view.
pub fn tree_directory(
    conn: &Connection,
    index: &IndexLocator,
    partition: &str,
    dir: &str,
    filters: &QueryFilters,
    sort_mode: SortMode,
) -> Result<Vec<DirEntry>> {
    let (sql, params) = tree_directory_sql(index, partition, dir, filters, sort_mode);
    directory_entries(conn, &sql, &params, dir)
}

/// Run a directory statement; each row is one entry under `dir`.
fn directory_entries(
    conn: &Connection,
    sql: &str,
    params: &[String],
    dir: &str,
) -> Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;

    while let Some(row) = rows.next()? {
        let component: String = row.get(0)?;
        let is_dir: bool = row.get(1)?;
        let total_size: i64 = row.get(2)?;
        let file_count: i64 = row.get(3)?;
        let latest_atime: i64 = row.get(4)?;
        entries.push(DirEntry {
            name: component.clone(),
            path: format!("{}/{}", dir, component),
            is_dir,
            total_size,
            file_count,
            latest_atime,
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    use crate::{S3Locator, index_glob};

    // The statements xdu-view built before they moved here, copied from the binary unchanged
    // but for `self` and the `, hive_partitioning = false` that `parquet_source` now puts after
    // every glob, so the local text each view sends is pinned byte for byte.

    fn old_root_files_sql(index_path: &Path, filters: &QueryFilters) -> String {
        let glob = index_glob(index_path, Some(ROOT_PARTITION));
        let where_clause = filters.to_full_where_clause();

        format!(
            r#"
            SELECT path, size, atime
            FROM read_parquet('{glob}', hive_partitioning = false)
            {where_clause}
            "#,
            glob = glob,
            where_clause = where_clause
        )
    }

    fn old_partitions_sql(
        index_path: &Path,
        filters: &QueryFilters,
        sort_mode: SortMode,
    ) -> String {
        let glob = index_glob(index_path, None);

        let filter_clause = filters.to_where_clause();
        let having_clause = if filter_clause.is_empty() {
            format!(
                "HAVING partition IS NOT NULL AND partition != '' AND partition != '{}'",
                ROOT_PARTITION
            )
        } else {
            format!(
                "HAVING partition IS NOT NULL AND partition != '' AND partition != '{}' AND SUM(CASE WHEN {} THEN 1 ELSE 0 END) > 0",
                ROOT_PARTITION, filter_clause
            )
        };

        if filters.is_active() {
            format!(
                r#"
                SELECT 
                    regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1) as partition,
                    SUM(CASE WHEN {filter} THEN size ELSE 0 END) as total_size,
                    MAX(CASE WHEN {filter} THEN atime ELSE 0 END) as latest_atime,
                    SUM(CASE WHEN {filter} THEN 1 ELSE 0 END) as file_count
                FROM read_parquet('{glob}', hive_partitioning = false, filename=true)
                GROUP BY partition
                {having}
                ORDER BY {order}
                "#,
                filter = filter_clause,
                glob = glob,
                having = having_clause,
                order = sort_mode.to_partition_order_by()
            )
        } else {
            format!(
                r#"
                SELECT 
                    regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1) as partition,
                    SUM(size) as total_size,
                    MAX(atime) as latest_atime,
                    COUNT(*) as file_count
                FROM read_parquet('{glob}', hive_partitioning = false, filename=true)
                GROUP BY partition
                HAVING partition IS NOT NULL AND partition != '' AND partition != '{root}'
                ORDER BY {order}
                "#,
                glob = glob,
                root = ROOT_PARTITION,
                order = sort_mode.to_partition_order_by()
            )
        }
    }

    fn old_partition_root_sql(index_path: &Path, partition: &str) -> String {
        let glob = index_glob(index_path, Some(partition));
        format!(
            "SELECT path FROM read_parquet('{}', hive_partitioning = false) ORDER BY length(path) LIMIT 1",
            glob
        )
    }

    fn old_directory_parts(
        dir: &str,
        filters: &QueryFilters,
        sort_mode: SortMode,
    ) -> (String, usize, &'static str) {
        let prefix = format!("{}/", dir);
        let prefix_len = prefix.len();
        let filter_clause = filters.to_where_clause();
        let file_filter = if filter_clause.is_empty() {
            format!("path LIKE '{}%'", prefix)
        } else {
            format!("path LIKE '{}%' AND {}", prefix, filter_clause)
        };
        let order_by = sort_mode.to_order_by(sort_mode == SortMode::Name);
        (file_filter, prefix_len, order_by)
    }

    fn old_list_directory_sql(
        index_path: &Path,
        partition: &str,
        dir: &str,
        filters: &QueryFilters,
        sort_mode: SortMode,
    ) -> String {
        let glob = index_glob(index_path, Some(partition));
        let (file_filter, prefix_len, order_by) = old_directory_parts(dir, filters, sort_mode);
        format!(
            r#"
            WITH files AS (
                SELECT 
                    path,
                    size,
                    atime
                FROM read_parquet('{glob}', hive_partitioning = false)
                WHERE {file_filter}
            ),
            components AS (
                SELECT 
                    path,
                    size,
                    atime,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 
                        THEN substr(path, {prefix_len} + 1, position('/' IN substr(path, {prefix_len} + 1)) - 1)
                        ELSE substr(path, {prefix_len} + 1)
                    END as component,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 THEN true
                        ELSE false
                    END as is_dir
                FROM files
            )
            SELECT 
                component,
                bool_or(is_dir) as is_dir,
                SUM(size) as total_size,
                COUNT(*) as file_count,
                MAX(atime) as latest_atime
            FROM components
            WHERE component != '' AND component IS NOT NULL
            GROUP BY component
            ORDER BY {order_by}
            "#,
            glob = glob,
            file_filter = file_filter,
            prefix_len = prefix_len,
            order_by = order_by
        )
    }

    fn old_tree_directory_sql(
        index_path: &Path,
        partition: &str,
        dir: &str,
        filters: &QueryFilters,
        sort_mode: SortMode,
    ) -> String {
        let glob = index_glob(index_path, Some(partition));
        let (file_filter, prefix_len, order_by) = old_directory_parts(dir, filters, sort_mode);
        format!(
            r#"
            WITH files AS (
                SELECT path, size, atime
                FROM read_parquet('{glob}', hive_partitioning = false)
                WHERE {file_filter}
            ),
            components AS (
                SELECT 
                    path, size, atime,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 
                        THEN substr(path, {prefix_len} + 1, position('/' IN substr(path, {prefix_len} + 1)) - 1)
                        ELSE substr(path, {prefix_len} + 1)
                    END as component,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 THEN true
                        ELSE false
                    END as is_dir
                FROM files
            )
            SELECT 
                component,
                bool_or(is_dir) as is_dir,
                SUM(size) as total_size,
                COUNT(*) as file_count,
                MAX(atime) as latest_atime
            FROM components
            WHERE component != '' AND component IS NOT NULL
            GROUP BY component
            ORDER BY {order_by}
            "#,
            glob = glob,
            file_filter = file_filter,
            prefix_len = prefix_len,
            order_by = order_by
        )
    }

    /// No filter, and one of every kind xdu-view accepts, with a quote in the pattern.
    fn filter_sets() -> Vec<QueryFilters> {
        let all = QueryFilters::new()
            .with_path_pattern(Some("*o'brien*.log".to_string()), false)
            .unwrap()
            .with_older_than(Some(30))
            .with_newer_than(Some(400))
            .with_min_size(Some("1K"))
            .unwrap()
            .with_max_size(Some("1G"))
            .unwrap();
        let regex = QueryFilters::new()
            .with_path_pattern(Some("\\.txt$".to_string()), true)
            .unwrap();
        vec![QueryFilters::new(), all, regex]
    }

    #[test]
    fn local_view_sql_is_byte_for_byte_what_xdu_view_sent() {
        let index_path = PathBuf::from("/index/scratch");
        let index = IndexLocator::Local(index_path.clone());
        let mut checked = 0;
        for filters in filter_sets() {
            assert_eq!(
                root_files_sql(&index, &filters),
                (old_root_files_sql(&index_path, &filters), vec![])
            );
            for partition in ["alice", ROOT_PARTITION, "o'brien"] {
                assert_eq!(
                    partition_root_sql(&index, partition),
                    (old_partition_root_sql(&index_path, partition), vec![])
                );
            }
            for sort_mode in SortMode::ALL {
                assert_eq!(
                    partitions_sql(&index, &filters, sort_mode),
                    (old_partitions_sql(&index_path, &filters, sort_mode), vec![])
                );
                for (partition, dir) in [
                    ("alice", "/data/scratch/alice"),
                    ("alice", "/data/scratch/alice/b/c"),
                    ("o'brien", "/data/scratch/o'brien/it's"),
                    ("données", "/data/scratch/données/e\u{301}t\u{e9}/\u{FFFD}"),
                ] {
                    // The offset is the one number that may differ from what xdu-view sent: it
                    // counts the prefix in code points, which for an ASCII prefix are its bytes.
                    let prefix = format!("{dir}/");
                    let (bytes, points) = (prefix.len(), prefix.chars().count());
                    if dir.is_ascii() {
                        assert_eq!(bytes, points, "{dir}");
                    } else {
                        assert_ne!(bytes, points, "{dir}");
                    }
                    let offset = |old: String| {
                        let from = format!("substr(path, {bytes} + 1");
                        assert_eq!(old.matches(&from).count(), 5, "{old}");
                        old.replace(&from, &format!("substr(path, {points} + 1"))
                    };
                    assert_eq!(
                        list_directory_sql(&index, partition, dir, &filters, sort_mode),
                        (
                            offset(old_list_directory_sql(
                                &index_path,
                                partition,
                                dir,
                                &filters,
                                sort_mode
                            )),
                            vec![]
                        )
                    );
                    assert_eq!(
                        tree_directory_sql(&index, partition, dir, &filters, sort_mode),
                        (
                            offset(old_tree_directory_sql(
                                &index_path,
                                partition,
                                dir,
                                &filters,
                                sort_mode
                            )),
                            vec![]
                        )
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 3 * 7 * 4);
    }

    #[test]
    fn directory_offset_counts_code_points() {
        // Two-, four- and three-byte scalars and a combining mark: 9 code points, 16 bytes.
        let dir = "/d/é𝄞\u{FFFD}e\u{301}";
        assert_eq!(dir.len() + 1, 16);
        assert_eq!(dir.chars().count() + 1, 9);
        let local = IndexLocator::Local(PathBuf::from("/index/scratch"));
        let s3 = IndexLocator::S3(S3Locator::parse(OsStr::new("s3://bucket/idx")).unwrap());
        for filters in filter_sets() {
            for sort_mode in SortMode::ALL {
                for build in [list_directory_sql, tree_directory_sql] {
                    for index in [&local, &s3] {
                        let (sql, params) = build(index, "p", dir, &filters, sort_mode);
                        assert!(sql.contains("substr(path, 9 + 1)"), "{sql}");
                        assert!(!sql.contains("16 + 1"), "{sql}");
                        if let IndexLocator::S3(_) = index {
                            assert_eq!(
                                params,
                                vec![
                                    "s3://bucket/idx/p/*.parquet".to_string(),
                                    format!("{dir}/%")
                                ]
                            );
                        } else {
                            assert!(params.is_empty(), "{params:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn s3_view_sql_binds_the_glob_and_the_directory_prefix() {
        // A quote in the prefix, the partition or a directory would end a literal; bound, none
        // of them reaches the text. The one option the text gains is the constant Hive one.
        let index = IndexLocator::S3(S3Locator::parse(OsStr::new("s3://bucket/it's")).unwrap());
        let glob = |partition: &str| format!("s3://bucket/it's/{partition}/*.parquet");
        let values = ["it's", "o'brien", "/data/x'y"];
        let assert_bound = |sql: &str| {
            for value in values {
                assert!(!sql.contains(value), "{value} reached the text: {sql}");
            }
            assert!(!sql.contains("s3://"), "the glob reached the text: {sql}");
        };

        for filters in filter_sets() {
            let (sql, params) = root_files_sql(&index, &filters);
            assert!(
                sql.contains("FROM read_parquet(?, hive_partitioning = false)"),
                "{sql}"
            );
            assert_eq!(params, vec![glob(ROOT_PARTITION)]);
            assert_bound(&sql);

            let (sql, params) = partition_root_sql(&index, "o'brien");
            assert_eq!(
                sql,
                "SELECT path FROM read_parquet(?, hive_partitioning = false) ORDER BY length(path) LIMIT 1"
            );
            assert_eq!(params, vec![glob("o'brien")]);

            for sort_mode in SortMode::ALL {
                let (sql, params) = partitions_sql(&index, &filters, sort_mode);
                assert!(
                    sql.contains("FROM read_parquet(?, hive_partitioning = false, filename=true)"),
                    "{sql}"
                );
                assert_eq!(params, vec![glob("*")]);
                assert_bound(&sql);

                for build in [list_directory_sql, tree_directory_sql] {
                    let (sql, params) = build(&index, "o'brien", "/data/x'y", &filters, sort_mode);
                    assert!(
                        sql.contains("FROM read_parquet(?, hive_partitioning = false)"),
                        "{sql}"
                    );
                    assert!(sql.contains("WHERE path LIKE ?"), "{sql}");
                    // The glob is the first placeholder and the prefix the second.
                    assert_eq!(params, vec![glob("o'brien"), "/data/x'y/%".to_string()]);
                    // Outside the filter text, whose own pattern may hold a `?`, every `?` is
                    // a placeholder.
                    let placeholders = sql.replace(&filters.to_where_clause(), "");
                    assert_eq!(placeholders.matches('?').count(), params.len(), "{sql}");
                    assert_bound(&sql);
                }
            }
        }
    }
}
