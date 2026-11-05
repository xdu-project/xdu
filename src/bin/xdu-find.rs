use std::io::{self, Write};

use anyhow::{Context, Result};
use clap::Parser;
use duckdb::params_from_iter;

use xdu::cli::XduFindArgs;
use xdu::s3::ProcessEnv;
use xdu::{
    IndexLocator, QueryFilters, S3Settings, index_completion_warning, index_version_error,
    marker_completion_warning, marker_version_error, parquet_source, read_completion_marker_s3,
    resolve_group, resolve_user,
};

fn main() -> Result<()> {
    let args = XduFindArgs::parse();

    // The index argument is classified on its bytes before anything else. An s3:// locator
    // resolves its credentials and opens its connection first, so a missing extension or
    // credential is the first thing an operator hears about; every other argument is a local
    // path and takes the path below unchanged, loading no extension.
    let (index, conn) = match IndexLocator::parse(args.index.as_os_str())? {
        IndexLocator::S3(locator) => {
            let settings = S3Settings::resolve(&ProcessEnv)?;
            let conn = xdu::engine::open_s3_connection(&settings)?;

            // The marker is read once, a round trip per statement, and both gates judge that
            // one read. A missing prefix, bucket or endpoint fails here, before any row.
            let marker = read_completion_marker_s3(&conn, &locator)?;
            let shown = locator.to_string();
            if let Some(error) = marker_version_error(&marker, &shown) {
                return Err(anyhow::anyhow!(error));
            }
            if let Some(warning) = marker_completion_warning(&marker, &shown) {
                eprintln!("{}", warning);
            }
            (IndexLocator::S3(locator), conn)
        }
        IndexLocator::Local(_) => {
            // Resolve index path
            let index_path = args
                .index
                .canonicalize()
                .with_context(|| format!("Index directory not found: {}", args.index.display()))?;

            // Refuse an index whose format version is unknown or unsupported before any
            // query: rows of an unknown layout must never be read, let alone trusted.
            if let Some(error) = index_version_error(&index_path) {
                return Err(anyhow::anyhow!(error));
            }

            // An index can be incomplete two ways — a run that never finished, or one that
            // finished under --allow-errors having skipped what it could not read. Either is
            // still queryable, so this warns rather than refusing; it goes to stderr so piped
            // results stay clean.
            if let Some(warning) = index_completion_warning(&index_path) {
                eprintln!("{}", warning);
            }

            // Connect to DuckDB (in-memory)
            let conn = xdu::engine::open_connection()?;
            (IndexLocator::Local(index_path), conn)
        }
    };

    // Every statement reads the index through this one source: the glob written into the
    // text for a local index, a bound parameter for an S3 one.
    let source = |filename: bool| parquet_source(&index, args.partition.as_deref(), filename);

    // Build filters using shared QueryFilters. A glob is translated here; an invalid
    // one fails before any query is built rather than at the database. Owner and
    // group names resolve to ids up front for the same reason: only integers
    // reach SQL, and an unresolvable name exits non-zero having printed no rows.
    let owner_uid = args
        .owner
        .as_deref()
        .map(resolve_user)
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?;
    let group_gid = args
        .group
        .as_deref()
        .map(resolve_group)
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?;
    let filters = QueryFilters::new()
        .with_path_pattern(args.pattern.clone(), args.regex)
        .map_err(|e| anyhow::anyhow!(e))?
        .with_older_than(args.older_than)
        .with_newer_than(args.newer_than)
        .with_min_size(args.min_size.as_deref())
        .map_err(|e| anyhow::anyhow!(e))?
        .with_max_size(args.max_size.as_deref())
        .map_err(|e| anyhow::anyhow!(e))?
        .with_owner_uid(owner_uid)
        .with_group_gid(group_gid)
        .with_mode(args.mode.as_deref())
        .map_err(|e| anyhow::anyhow!(e))?
        .with_mtime_older_than(args.mtime_older_than)
        .with_mtime_newer_than(args.mtime_newer_than);

    let where_clause = filters.to_full_where_clause();

    let limit_clause = if let Some(n) = args.limit {
        format!("LIMIT {}", n)
    } else {
        String::new()
    };

    // Build and execute query based on format
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Handle --top mode: show top N partitions by file count
    if let Some(n) = args.top {
        // Extract partition name from the path (parent directory of the parquet file)
        // The glob pattern is index/*/*.parquet, so we extract the partition from the path
        let (from, params) = source(true);
        let sql = format!(
            "SELECT 
                regexp_extract(filename, '.*/([^/]+)/[^/]+\\.parquet$', 1) as partition,
                COUNT(*) as file_count
            FROM {} {}
            GROUP BY partition
            ORDER BY file_count DESC
            LIMIT {}",
            from, where_clause, n
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(&params))?;
        while let Some(row) = rows.next()? {
            let partition: String = row.get(0)?;
            writeln!(out, "{}", partition)?;
        }
        return Ok(());
    }

    // Handle count mode
    if args.count {
        let (from, params) = source(false);
        let sql = format!("SELECT COUNT(*) FROM {} {}", from, where_clause);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(&params))?;
        if let Some(row) = rows.next()? {
            let count: i64 = row.get(0)?;
            writeln!(out, "{}", count)?;
        }
        return Ok(());
    }

    match args.format.as_str() {
        "path" => {
            let (from, params) = source(false);
            let sql = format!(
                "SELECT path FROM {} {} {}",
                from, where_clause, limit_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(&params))?;
            while let Some(row) = rows.next()? {
                let path: String = row.get(0)?;
                writeln!(out, "{}", path)?;
            }
        }
        "size" => {
            let (from, params) = source(false);
            let sql = format!(
                "SELECT path, size FROM {} {} ORDER BY size DESC {}",
                from, where_clause, limit_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(&params))?;
            while let Some(row) = rows.next()? {
                let path: String = row.get(0)?;
                let size: i64 = row.get(1)?;
                writeln!(out, "{}\t{}", size, path)?;
            }
        }
        "atime" => {
            let (from, params) = source(false);
            let sql = format!(
                "SELECT path, atime FROM {} {} ORDER BY atime ASC {}",
                from, where_clause, limit_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(&params))?;
            while let Some(row) = rows.next()? {
                let path: String = row.get(0)?;
                let atime: i64 = row.get(1)?;
                writeln!(out, "{}\t{}", atime, path)?;
            }
        }
        "csv" => {
            let (from, params) = source(false);
            let sql = format!(
                "SELECT path, size, uid, gid, mode, atime, mtime, ctime FROM {} {} {}",
                from, where_clause, limit_clause
            );
            writeln!(out, "path,size,uid,gid,mode,atime,mtime,ctime")?;
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(&params))?;
            while let Some(row) = rows.next()? {
                let path: String = row.get(0)?;
                let size: i64 = row.get(1)?;
                let uid: i64 = row.get(2)?;
                let gid: i64 = row.get(3)?;
                let mode: i64 = row.get(4)?;
                let atime: i64 = row.get(5)?;
                let mtime: i64 = row.get(6)?;
                let ctime: i64 = row.get(7)?;
                // Escape commas and quotes in path for CSV
                if path.contains(',') || path.contains('"') {
                    writeln!(
                        out,
                        "\"{}\",{},{},{},{},{},{},{}",
                        path.replace('"', "\"\""),
                        size,
                        uid,
                        gid,
                        mode,
                        atime,
                        mtime,
                        ctime
                    )?;
                } else {
                    writeln!(
                        out,
                        "{},{},{},{},{},{},{},{}",
                        path, size, uid, gid, mode, atime, mtime, ctime
                    )?;
                }
            }
        }
        "json" => {
            let (from, params) = source(false);
            let sql = format!(
                "SELECT path, size, uid, gid, mode, atime, mtime, ctime FROM {} {} {}",
                from, where_clause, limit_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(&params))?;
            let mut first = true;
            writeln!(out, "[")?;
            while let Some(row) = rows.next()? {
                let path: String = row.get(0)?;
                let size: i64 = row.get(1)?;
                let uid: i64 = row.get(2)?;
                let gid: i64 = row.get(3)?;
                let mode: i64 = row.get(4)?;
                let atime: i64 = row.get(5)?;
                let mtime: i64 = row.get(6)?;
                let ctime: i64 = row.get(7)?;
                if !first {
                    writeln!(out, ",")?;
                }
                first = false;
                // Escape JSON special characters in path
                let escaped_path = path
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t");
                write!(
                    out,
                    "  {{\"path\":\"{}\",\"size\":{},\"uid\":{},\"gid\":{},\"mode\":{},\"atime\":{},\"mtime\":{},\"ctime\":{}}}",
                    escaped_path, size, uid, gid, mode, atime, mtime, ctime
                )?;
            }
            writeln!(out)?;
            writeln!(out, "]")?;
        }
        _ => {
            anyhow::bail!(
                "Unknown format: {}. Use: path, size, atime, csv, json",
                args.format
            );
        }
    }

    Ok(())
}
