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
    ModePredicate, QueryFilters, SizeMode, deterministic_limit_clause, index_completion_warning,
    index_glob, index_version_error, resolve_group, resolve_user,
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
/// size measure the index records, the completion warning the caller owes stderr, and the
/// rows in ascending path order.
#[derive(Debug)]
pub struct Selection {
    pub index: PathBuf,
    pub filters: QueryFilters,
    /// The measure the index's `size` column holds, when the index states it; `None` when it
    /// does not, and then a live size check must hold under every measure.
    pub size_mode: Option<SizeMode>,
    pub warning: Option<String>,
    pub rows: Vec<Row>,
}

/// The live metadata of the object a tool is about to act on, read with `lstat` semantics:
/// the object at the path itself, never a symlink's target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live {
    pub regular: bool,
    /// The file's length in bytes (`st_size`).
    pub length: i64,
    /// The space the file occupies (`st_blocks` × 512).
    pub disk_usage: i64,
    pub uid: u32,
    pub gid: u32,
    /// The permission bits, masked to `0o7777` as the index stores them.
    pub mode: u32,
    pub atime: i64,
    pub mtime: i64,
}

/// Whether a file still matches the filters it was selected by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Holds,
    /// No longer a regular file: a symlink, directory, FIFO, socket or device now holds the
    /// path.
    NotRegular,
    /// The first filter that no longer holds, as the skip reason a tool reports.
    Changed(&'static str),
}

/// The live sizes a size filter is tested on: the index's own measure when it states one, and
/// both the length and the disk usage when it does not, so a file that changed since the
/// crawl fails closed whichever measure selected it.
fn live_sizes(live: &Live, size_mode: Option<SizeMode>) -> Vec<i64> {
    match size_mode {
        Some(mode) => {
            let disk = u64::try_from(live.disk_usage).unwrap_or(0);
            let length = u64::try_from(live.length).unwrap_or(0);
            vec![i64::try_from(mode.calculate(disk, length)).unwrap_or(i64::MAX)]
        }
        None => vec![live.length, live.disk_usage],
    }
}

/// `--safe`'s re-check: every filter `to_conditions` writes as SQL, evaluated against the
/// object's live metadata with the thresholds fixed when the filters were built.
///
/// The path pattern and the partition hold by construction: the path is the row's own. The
/// comparisons mirror the SQL exactly (`size >= min`, `atime < older`, `atime >= newer`, and
/// so on), so a file whose metadata is unchanged since the crawl always holds.
pub fn holds_live(filters: &QueryFilters, size_mode: Option<SizeMode>, live: &Live) -> Verdict {
    if !live.regular {
        return Verdict::NotRegular;
    }
    let sizes = live_sizes(live, size_mode);
    if let Some(min) = filters.min_size
        && sizes.iter().any(|size| *size < min)
    {
        return Verdict::Changed("no longer matches --min-size");
    }
    if let Some(max) = filters.max_size
        && sizes.iter().any(|size| *size > max)
    {
        return Verdict::Changed("no longer matches --max-size");
    }
    if let Some(t) = filters.older_than
        && live.atime >= t
    {
        return Verdict::Changed("no longer matches --older-than");
    }
    if let Some(t) = filters.newer_than
        && live.atime < t
    {
        return Verdict::Changed("no longer matches --newer-than");
    }
    if let Some(uid) = filters.owner_uid
        && live.uid != uid
    {
        return Verdict::Changed("no longer matches --owner");
    }
    if let Some(gid) = filters.group_gid
        && live.gid != gid
    {
        return Verdict::Changed("no longer matches --group");
    }
    if let Some(pred) = filters.mode {
        let mode = live.mode & 0o7777;
        let holds = match pred {
            ModePredicate::Exact(bits) => mode == bits,
            ModePredicate::Any(bits) => mode & bits != 0,
            ModePredicate::All(bits) => mode & bits == bits,
        };
        if !holds {
            return Verdict::Changed("no longer matches --mode");
        }
    }
    if let Some(t) = filters.mtime_older_than
        && live.mtime >= t
    {
        return Verdict::Changed("no longer matches --mtime-older-than");
    }
    if let Some(t) = filters.mtime_newer_than
        && live.mtime < t
    {
        return Verdict::Changed("no longer matches --mtime-newer-than");
    }
    Verdict::Holds
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

/// The table function a selection reads: the bound glob, with Hive partitioning off.
///
/// DuckDB otherwise detects `key=value` directory names in the paths it globs and turns each
/// into a column that shadows the file's own: a partition named `size=0`, or an index stored
/// under `…/mtime=0/`, would make every row's `size` or `mtime` read 0, and the filters would
/// select files other than the ones the index describes.
const SELECTION_SOURCE: &str = "read_parquet(?, hive_partitioning = false)";

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
        format!("SELECT path, size, uid, gid, mode, atime, mtime FROM {SELECTION_SOURCE}");
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
        // No index format records the crawl's size mode, so a live size check cannot know
        // which measure a row holds and tests both.
        size_mode: None,
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
            "SELECT path, size, uid, gid, mode, atime, mtime \
             FROM read_parquet(?, hive_partitioning = false) \
             WHERE regexp_matches(path, ?) ORDER BY path"
        );
        assert_eq!(params, vec![glob.to_string(), "it's$".to_string()]);
        assert!(!sql.contains('\''), "no value reaches the text: {sql}");

        let (sql, params) = selection_sql(glob, &QueryFilters::default(), None);
        assert_eq!(
            sql,
            "SELECT path, size, uid, gid, mode, atime, mtime \
             FROM read_parquet(?, hive_partitioning = false) ORDER BY path"
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

    /// A regular file whose length and disk usage agree, so every size measure reads `size`.
    fn live(size: i64) -> Live {
        Live {
            regular: true,
            length: size,
            disk_usage: size,
            uid: 501,
            gid: 20,
            mode: 0o644,
            atime: 1_650_000_000,
            mtime: 1_650_000_000,
        }
    }

    /// Whether DuckDB keeps a one-row table holding `row` under the filters' own SQL.
    fn sql_holds(conn: &duckdb::Connection, filters: &QueryFilters, row: &Live) -> bool {
        let sql = format!(
            "SELECT count(*) FROM (VALUES (CAST(? AS BIGINT), CAST(? AS BIGINT), \
             CAST(? AS BIGINT), CAST(? AS BIGINT), CAST(? AS BIGINT), CAST(? AS BIGINT))) \
             AS r(size, uid, gid, mode, atime, mtime) WHERE {}",
            filters.to_where_clause()
        );
        let n: i64 = conn
            .query_row(
                &sql,
                duckdb::params![
                    row.length,
                    i64::from(row.uid),
                    i64::from(row.gid),
                    i64::from(row.mode),
                    row.atime,
                    row.mtime
                ],
                |r| r.get(0),
            )
            .unwrap();
        n == 1
    }

    #[test]
    fn the_live_check_agrees_with_the_sql_on_every_boundary() {
        let conn = crate::engine::open_connection().unwrap();
        let t = 1_700_000_000_i64;
        type Set = fn(&mut Live, i64);
        let set_size: Set = |l, v| {
            l.length = v;
            l.disk_usage = v;
        };
        let set_atime: Set = |l, v| l.atime = v;
        let set_mtime: Set = |l, v| l.mtime = v;
        let set_uid: Set = |l, v| l.uid = v as u32;
        let set_gid: Set = |l, v| l.gid = v as u32;
        let none = QueryFilters::default();
        let clocks: Vec<(QueryFilters, i64, Set)> = vec![
            (
                QueryFilters {
                    min_size: Some(4096),
                    ..none.clone()
                },
                4096,
                set_size,
            ),
            (
                QueryFilters {
                    max_size: Some(4096),
                    ..none.clone()
                },
                4096,
                set_size,
            ),
            (
                QueryFilters {
                    older_than: Some(t),
                    ..none.clone()
                },
                t,
                set_atime,
            ),
            (
                QueryFilters {
                    newer_than: Some(t),
                    ..none.clone()
                },
                t,
                set_atime,
            ),
            (
                QueryFilters {
                    mtime_older_than: Some(t),
                    ..none.clone()
                },
                t,
                set_mtime,
            ),
            (
                QueryFilters {
                    mtime_newer_than: Some(t),
                    ..none.clone()
                },
                t,
                set_mtime,
            ),
            (
                QueryFilters {
                    owner_uid: Some(501),
                    ..none.clone()
                },
                501,
                set_uid,
            ),
            (
                QueryFilters {
                    group_gid: Some(20),
                    ..none.clone()
                },
                20,
                set_gid,
            ),
        ];
        for (filters, at, set) in &clocks {
            let mut seen = (false, false);
            for v in [at - 1, *at, at + 1] {
                let mut row = live(0);
                set(&mut row, v);
                let sql = sql_holds(&conn, filters, &row);
                let verdict = holds_live(filters, None, &row);
                assert_eq!(
                    verdict == Verdict::Holds,
                    sql,
                    "{filters:?} at {v}: {verdict:?}"
                );
                if sql { seen.0 = true } else { seen.1 = true }
            }
            assert!(seen.0 && seen.1, "{filters:?}: the grid must hold and fail");
        }

        for pred in [
            ModePredicate::Exact(0o644),
            ModePredicate::Any(0o022),
            ModePredicate::All(0o640),
            ModePredicate::All(0o4000),
        ] {
            let bits = match pred {
                ModePredicate::Exact(b) | ModePredicate::Any(b) | ModePredicate::All(b) => b,
            };
            let filters = QueryFilters {
                mode: Some(pred),
                ..none.clone()
            };
            let mut seen = (false, false);
            for m in [
                bits - 1,
                bits,
                bits + 1,
                0,
                0o600,
                0o644,
                0o664,
                0o4755,
                0o7777,
            ] {
                let mut row = live(0);
                row.mode = m;
                let sql = sql_holds(&conn, &filters, &row);
                assert_eq!(
                    holds_live(&filters, None, &row) == Verdict::Holds,
                    sql,
                    "{pred} at {m:o}"
                );
                if sql { seen.0 = true } else { seen.1 = true }
            }
            assert!(seen.0 && seen.1, "{pred}: the grid must hold and fail");
        }

        // Every filter at once: the unchanged row holds, and moving any one column across its
        // boundary fails both the SQL and the live check.
        let all = QueryFilters {
            min_size: Some(100),
            max_size: Some(200),
            older_than: Some(t),
            newer_than: Some(t - 1000),
            owner_uid: Some(501),
            group_gid: Some(20),
            mode: Some(ModePredicate::Exact(0o644)),
            mtime_older_than: Some(t),
            mtime_newer_than: Some(t - 1000),
            ..none.clone()
        };
        let mut base = live(150);
        base.atime = t - 1;
        base.mtime = t - 1000;
        assert!(sql_holds(&conn, &all, &base));
        assert_eq!(holds_live(&all, None, &base), Verdict::Holds);
        let moves: [(Set, i64); 8] = [
            (set_size, 99),
            (set_size, 201),
            (set_atime, t),
            (set_atime, t - 1001),
            (set_mtime, t),
            (set_mtime, t - 1001),
            (set_uid, 502),
            (set_gid, 21),
        ];
        for (set, v) in moves {
            let mut row = base;
            set(&mut row, v);
            assert!(!sql_holds(&conn, &all, &row), "{row:?}");
            assert!(
                matches!(holds_live(&all, None, &row), Verdict::Changed(_)),
                "{row:?}"
            );
        }
    }

    #[test]
    fn size_uses_the_measure_that_matches_the_indexed_size() {
        // 1000 bytes long, occupying one 4 KiB block.
        let row = Live {
            length: 1000,
            disk_usage: 4096,
            ..live(0)
        };
        let min = QueryFilters {
            min_size: Some(2048),
            ..QueryFilters::default()
        };
        let max = QueryFilters {
            max_size: Some(2048),
            ..QueryFilters::default()
        };
        let cases = [
            (
                Some(SizeMode::DiskUsage),
                Verdict::Holds,
                Verdict::Changed("no longer matches --max-size"),
            ),
            (
                Some(SizeMode::ApparentSize),
                Verdict::Changed("no longer matches --min-size"),
                Verdict::Holds,
            ),
            (
                Some(SizeMode::BlockRounded(4096)),
                Verdict::Holds,
                Verdict::Changed("no longer matches --max-size"),
            ),
            (
                Some(SizeMode::BlockRounded(512)),
                Verdict::Changed("no longer matches --min-size"),
                Verdict::Holds,
            ),
            // An index that does not state its measure: the filter must hold under both.
            (
                None,
                Verdict::Changed("no longer matches --min-size"),
                Verdict::Changed("no longer matches --max-size"),
            ),
        ];
        for (mode, with_min, with_max) in cases {
            assert_eq!(
                holds_live(&min, mode, &row),
                with_min,
                "{mode:?} --min-size"
            );
            assert_eq!(
                holds_live(&max, mode, &row),
                with_max,
                "{mode:?} --max-size"
            );
        }
        // When both measures satisfy the filter, every reading holds.
        let wide = QueryFilters {
            min_size: Some(512),
            max_size: Some(8192),
            ..QueryFilters::default()
        };
        for mode in [
            None,
            Some(SizeMode::DiskUsage),
            Some(SizeMode::ApparentSize),
        ] {
            assert_eq!(holds_live(&wide, mode, &row), Verdict::Holds, "{mode:?}");
        }
    }

    #[test]
    fn a_file_that_shrank_below_min_size_fails_the_live_check() {
        let filters = QueryFilters {
            min_size: Some(2048),
            ..QueryFilters::default()
        };
        // Indexed at 4096 bytes and selected; since then truncated to 100 bytes, while the
        // filesystem still reports one 4 KiB block.
        let conn = crate::engine::open_connection().unwrap();
        assert!(sql_holds(&conn, &filters, &live(4096)));
        let shrunk = Live {
            length: 100,
            disk_usage: 4096,
            ..live(0)
        };
        assert_eq!(
            holds_live(&filters, None, &shrunk),
            Verdict::Changed("no longer matches --min-size")
        );
        let emptied = Live {
            length: 0,
            disk_usage: 0,
            ..live(0)
        };
        assert_eq!(
            holds_live(&filters, None, &emptied),
            Verdict::Changed("no longer matches --min-size")
        );
        assert_eq!(holds_live(&filters, None, &live(4096)), Verdict::Holds);
    }

    #[test]
    fn a_non_regular_file_never_holds() {
        let matching = QueryFilters {
            min_size: Some(1),
            mode: Some(ModePredicate::Exact(0o644)),
            ..QueryFilters::default()
        };
        for filters in [QueryFilters::default(), matching] {
            let row = Live {
                regular: false,
                ..live(4096)
            };
            for mode in [None, Some(SizeMode::ApparentSize)] {
                assert_eq!(
                    holds_live(&filters, mode, &row),
                    Verdict::NotRegular,
                    "{filters:?}"
                );
            }
            assert_eq!(holds_live(&filters, None, &live(4096)), Verdict::Holds);
        }
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
