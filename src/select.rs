//! The selection half of select-then-act: which indexed files a bulk tool acts on.
//!
//! A selection passes the index gates, builds `xdu-find`'s filters, and reads the matching rows
//! through one bound statement that is always ordered by path. It knows nothing about a
//! destination, a thread pool or the filesystem beyond the index itself, so a tool that consumes
//! one ordered stream (an archiver) can take it without the parallel executor.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use duckdb::params_from_iter;

use crate::cli::SelectArgs;
use crate::{
    QueryFilters, deterministic_limit_clause, index_completion_warning, index_glob,
    index_version_error, resolve_group, resolve_user,
};

/// One selected index row: the columns a select-then-act tool needs to act and to re-check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    /// The size the crawler recorded, in whichever measure the index was built with.
    pub size: i64,
    pub uid: i64,
    pub gid: i64,
    pub mode: i64,
    pub atime: i64,
    pub mtime: i64,
}

/// The outcome of `select`: the canonical index, the filters the rows were chosen by, the
/// completion warning the caller owes stderr, and the rows in ascending path order.
#[derive(Debug)]
pub struct Selection {
    pub index: PathBuf,
    pub filters: QueryFilters,
    pub warning: Option<String>,
    pub rows: Vec<Row>,
}

/// The refusal of an `s3://` index, as `xdu-rm` refuses one.
///
/// Judged on the raw argument bytes, so it runs before anything that could touch the network,
/// the extension directory or the local filesystem.
pub fn refuse_s3(raw: &OsStr, tool: &str) -> Result<(), String> {
    if crate::s3::is_s3_locator(raw) {
        Err(format!("{tool}: S3 indices are not supported by {tool}"))
    } else {
        Ok(())
    }
}

/// `xdu-find`'s filter chain over the shared flags.
///
/// Owner and group names resolve here, on the calling thread, because the lookups are not
/// reentrant; every threshold is fixed now, so a later live re-check compares against the same
/// instant the query did.
pub fn build_filters(args: &SelectArgs) -> Result<QueryFilters> {
    let owner_uid = args
        .owner
        .as_deref()
        .map(resolve_user)
        .transpose()
        .map_err(|e| anyhow!(e))?;
    let group_gid = args
        .group
        .as_deref()
        .map(resolve_group)
        .transpose()
        .map_err(|e| anyhow!(e))?;
    Ok(QueryFilters::new()
        .with_path_pattern(args.pattern.clone(), args.regex)
        .map_err(|e| anyhow!(e))?
        .with_older_than(args.older_than)
        .with_newer_than(args.newer_than)
        .with_min_size(args.min_size.as_deref())
        .map_err(|e| anyhow!(e))?
        .with_max_size(args.max_size.as_deref())
        .map_err(|e| anyhow!(e))?
        .with_owner_uid(owner_uid)
        .with_group_gid(group_gid)
        .with_mode(args.mode.as_deref())
        .map_err(|e| anyhow!(e))?
        .with_mtime_older_than(args.mtime_older_than)
        .with_mtime_newer_than(args.mtime_newer_than))
}

/// Characters that make a bound `read_parquet` argument match something other than itself.
///
/// Binding keeps a quote from becoming SQL, but DuckDB still globs the bound string: an index
/// at `idx[ab]` reads a sibling `idxa`, and `st*r` reads `stXr` too, with no error. A lone `\`
/// is glob syntax with no escape. Braces and a lone `]` are literal and pass.
const GLOB_METACHARACTERS: [char; 4] = ['*', '?', '[', '\\'];

/// The glob a selection reads, refused when it could name files other than the operator's.
///
/// The index path must be UTF-8 and free of glob metacharacters. A partition name must be one
/// existing directory under the index: not empty, `.` or `..`, no `/`, no metacharacter. The
/// layout becomes a glob only through `index_glob`.
pub fn checked_glob(index: &Path, partition: Option<&str>) -> Result<String, String> {
    let shown = index.display();
    let Some(text) = index.to_str() else {
        return Err(format!(
            "index path {shown} is not valid UTF-8, which the index reader cannot name exactly"
        ));
    };
    if let Some(c) = text.chars().find(|c| GLOB_METACHARACTERS.contains(c)) {
        return Err(format!(
            "index path {text} contains the glob character '{c}', which could select another \
             index; rename or link the index to a path without * ? [ or \\"
        ));
    }
    if let Some(name) = partition {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(format!(
                "partition name '{name}' does not name one directory under the index"
            ));
        }
        if let Some(c) = name.chars().find(|c| GLOB_METACHARACTERS.contains(c)) {
            return Err(format!(
                "partition name '{name}' contains the glob character '{c}', which could select \
                 other partitions"
            ));
        }
        let is_dir = fs::symlink_metadata(index.join(name))
            .map(|m| m.is_dir())
            .unwrap_or(false);
        if !is_dir {
            return Err(format!("no partition {name} in {text}"));
        }
    }
    Ok(index_glob(index, partition))
}

/// The selection statement and its bound values, as a pure function of the filters.
///
/// The glob is the first bound value and the pattern, when set, the second; every other
/// predicate is `QueryFilters::to_conditions` on a copy without the pattern, so the readers and
/// this statement share one definition of each filter. The order is unconditional: a limit
/// reuses `deterministic_limit_clause`, which already reads `ORDER BY path LIMIT n`, and the
/// unlimited statement orders too, so a refusal can name the first offending path and a
/// preview lists what the run will act on in the order it acts.
pub fn selection_sql(
    glob: &str,
    filters: &QueryFilters,
    limit: Option<usize>,
) -> (String, Vec<String>) {
    let mut params = vec![glob.to_string()];
    let mut conditions = Vec::new();
    if let Some(pattern) = &filters.pattern {
        conditions.push("regexp_matches(path, ?)".to_string());
        params.push(pattern.clone());
    }
    let mut rest = filters.clone();
    rest.pattern = None;
    rest.pattern_display = None;
    conditions.extend(rest.to_conditions());

    let mut sql =
        String::from("SELECT path, size, uid, gid, mode, atime, mtime FROM read_parquet(?)");
    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    let tail = match limit {
        Some(_) => deterministic_limit_clause(limit),
        None => "ORDER BY path".to_string(),
    };
    sql.push(' ');
    sql.push_str(&tail);
    (sql, params)
}

/// Run the gates and the query: the rows a tool will act on, in ascending path order.
///
/// The caller refuses an `s3://` index first (`refuse_s3`). Here the index is canonicalized,
/// an unknown format version refuses, an incomplete index yields a warning for the caller to
/// print, the filters are built, and the glob is checked before any SQL exists.
pub fn select(args: &SelectArgs) -> Result<Selection> {
    let index = args
        .index
        .canonicalize()
        .with_context(|| format!("Index directory not found: {}", args.index.display()))?;
    if let Some(error) = index_version_error(&index) {
        return Err(anyhow!(error));
    }
    let warning = index_completion_warning(&index);
    let filters = build_filters(args)?;
    let glob = checked_glob(&index, args.partition.as_deref()).map_err(|e| anyhow!(e))?;
    let (sql, params) = selection_sql(&glob, &filters, args.limit);

    let conn = crate::engine::open_connection()?;
    let mut stmt = conn.prepare(&sql)?;
    let mut found = stmt.query(params_from_iter(&params))?;
    let mut rows = Vec::new();
    while let Some(row) = found.next()? {
        rows.push(Row {
            path: row.get(0)?,
            size: row.get(1)?,
            uid: row.get(2)?,
            gid: row.get(3)?,
            mode: row.get(4)?,
            atime: row.get(5)?,
            mtime: row.get(6)?,
        });
    }
    Ok(Selection {
        index,
        filters,
        warning,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModePredicate;
    use std::os::unix::ffi::OsStrExt;

    fn every_filter() -> QueryFilters {
        QueryFilters {
            pattern: Some("^/data/.*\\.log$".to_string()),
            pattern_display: Some("/data/*.log".to_string()),
            min_size: Some(1024),
            max_size: Some(1 << 30),
            older_than: Some(1_700_000_000),
            newer_than: Some(1_600_000_000),
            owner_uid: Some(501),
            group_gid: Some(20),
            mode: Some(ModePredicate::Any(0o002)),
            mtime_older_than: Some(1_700_000_001),
            mtime_newer_than: Some(1_600_000_001),
        }
    }

    #[test]
    fn the_selection_sql_binds_the_glob_and_the_pattern() {
        let glob = "/idx'q/al'ice/*.parquet";
        let filters = QueryFilters {
            pattern: Some("it's$".to_string()),
            ..QueryFilters::default()
        };
        let (sql, params) = selection_sql(glob, &filters, None);
        assert_eq!(
            sql,
            "SELECT path, size, uid, gid, mode, atime, mtime FROM read_parquet(?) \
             WHERE regexp_matches(path, ?) ORDER BY path"
        );
        assert_eq!(params, vec![glob.to_string(), "it's$".to_string()]);
        assert!(!sql.contains('\''), "no value reaches the text: {sql}");

        let (sql, params) = selection_sql(glob, &QueryFilters::default(), None);
        assert_eq!(
            sql,
            "SELECT path, size, uid, gid, mode, atime, mtime FROM read_parquet(?) ORDER BY path"
        );
        assert_eq!(params, vec![glob.to_string()]);
    }

    #[test]
    fn the_selection_always_orders_by_path_once() {
        for filters in [QueryFilters::default(), every_filter()] {
            for limit in [None, Some(0), Some(7)] {
                let (sql, _) = selection_sql("/i/*/*.parquet", &filters, limit);
                assert_eq!(sql.matches("ORDER BY").count(), 1, "{sql}");
                assert_eq!(sql.matches("ORDER BY path").count(), 1, "{sql}");
                match limit {
                    Some(n) => assert!(sql.ends_with(&format!("ORDER BY path LIMIT {n}")), "{sql}"),
                    None => {
                        assert!(sql.ends_with("ORDER BY path"), "{sql}");
                        assert!(!sql.contains("LIMIT"), "{sql}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_selection_reuses_query_filters_predicates() {
        let filters = every_filter();
        let (sql, params) = selection_sql("/i/*/*.parquet", &filters, None);
        let mut rest = filters.clone();
        rest.pattern = None;
        let expected = rest.to_conditions();
        assert_eq!(expected.len(), 9, "every non-pattern filter is set");
        for condition in &expected {
            assert_eq!(
                sql.matches(condition.as_str()).count(),
                1,
                "{condition} in {sql}"
            );
        }
        assert!(sql.contains("regexp_matches(path, ?)"));
        assert!(
            !sql.contains("data/"),
            "the pattern is bound, not written: {sql}"
        );
        assert_eq!(params[1], filters.pattern.unwrap());
        let where_part = sql
            .split(" WHERE ")
            .nth(1)
            .and_then(|w| w.strip_suffix(" ORDER BY path"))
            .unwrap();
        let mut conditions = vec!["regexp_matches(path, ?)".to_string()];
        conditions.extend(expected);
        assert_eq!(where_part, conditions.join(" AND "));
    }

    #[test]
    fn an_index_path_with_glob_metacharacters_is_refused() {
        for bad in ["/x/idx[ab]", "/x/st*r", "/x/q?m", "/x/b\\s", "/x/x[y"] {
            let err = checked_glob(Path::new(bad), None).unwrap_err();
            assert!(err.contains("glob character"), "{bad}: {err}");
        }
        for good in ["/x/idx'q", "/x/b{x,y}", "/x/a]b", "/x/plain"] {
            let glob = checked_glob(Path::new(good), None).unwrap();
            assert_eq!(glob, format!("{good}/*/*.parquet"));
        }
        let lossy = Path::new(OsStr::from_bytes(b"/x/\xffidx"));
        assert!(checked_glob(lossy, None).unwrap_err().contains("UTF-8"));
    }

    #[test]
    fn a_partition_name_that_could_leave_the_index_is_refused() {
        let dir = tempfile::TempDir::new().unwrap();
        let index = dir.path().canonicalize().unwrap();
        fs::create_dir(index.join("al'ice")).unwrap();
        fs::create_dir(index.join("bob")).unwrap();
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "../bob",
            "p/../../idx",
            "b?b",
            "b*",
            "[b]ob",
            "b\\ob",
        ] {
            assert!(
                checked_glob(&index, Some(bad)).is_err(),
                "{bad:?} was accepted"
            );
        }
        let missing = checked_glob(&index, Some("carol")).unwrap_err();
        assert!(missing.starts_with("no partition carol in "), "{missing}");
        assert_eq!(
            checked_glob(&index, Some("al'ice")).unwrap(),
            format!("{}/al'ice/*.parquet", index.display())
        );
        assert_eq!(
            checked_glob(&index, Some("bob")).unwrap(),
            format!("{}/bob/*.parquet", index.display())
        );
    }
}
