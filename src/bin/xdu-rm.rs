use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;

use xdu::cli::XduRmArgs;
use xdu::{
    QueryFilters, deterministic_limit_clause, index_completion_warning, index_glob,
    index_version_error, parse_size,
};

/// File info from the index query
#[allow(dead_code)]
struct FileInfo {
    path: String,
    size: i64,
    atime: i64,
}

fn main() -> Result<()> {
    let args = XduRmArgs::parse();

    // Configure thread pool
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build_global()
        .ok(); // Ignore error if pool already initialized

    // Resolve index path
    let index_path = args
        .index
        .canonicalize()
        .with_context(|| format!("Index directory not found: {}", args.index.display()))?;

    // Refuse an index whose format version is unknown or unsupported before any
    // selection: no deletion set may come from an unknown layout.
    if let Some(error) = index_version_error(&index_path) {
        return Err(anyhow::anyhow!(error));
    }

    // An index that never finished, or that finished under --allow-errors, is missing rows —
    // which for a deletion tool means files it will not consider. Worth saying out loud
    // before anything is unlinked, the more so because whoever tolerated those errors at
    // build time is rarely whoever is running this now.
    if let Some(warning) = index_completion_warning(&index_path) {
        eprintln!("{}", warning);
    }

    let glob_pattern = index_glob(&index_path, args.partition.as_deref());

    // Connect to DuckDB (in-memory)
    let conn = xdu::engine::open_connection()?;

    // Build filters using shared QueryFilters. A glob is translated here; an invalid
    // one fails before any deletion set is selected rather than at the database.
    let filters = QueryFilters::new()
        .with_path_pattern(args.pattern.clone(), args.regex)
        .map_err(|e| anyhow::anyhow!(e))?
        .with_older_than(args.older_than)
        .with_newer_than(args.newer_than)
        .with_min_size(args.min_size.as_deref())
        .map_err(|e| anyhow::anyhow!(e))?
        .with_max_size(args.max_size.as_deref())
        .map_err(|e| anyhow::anyhow!(e))?;

    let where_clause = filters.to_full_where_clause();

    // A LIMIT without a deterministic ORDER BY returns an arbitrary subset, so --dry-run and
    // the real run could delete different files. Pair the limit with `ORDER BY path` (the
    // unique key) so the preview and the real deletion always select identical rows.
    let limit_clause = deterministic_limit_clause(args.limit);

    // Query for matching files
    let sql = format!(
        "SELECT path, size, atime FROM read_parquet('{}') {} {}",
        glob_pattern, where_clause, limit_clause
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;

    let mut files: Vec<FileInfo> = Vec::new();
    while let Some(row) = rows.next()? {
        files.push(FileInfo {
            path: row.get(0)?,
            size: row.get(1)?,
            atime: row.get(2)?,
        });
    }

    if files.is_empty() {
        println!("No matching files found.");
        return Ok(());
    }

    // Dry run mode: just print paths
    if args.dry_run {
        for file in &files {
            println!("{}", file.path);
        }
        println!("\n{} file(s) would be deleted.", files.len());
        return Ok(());
    }

    // Confirmation prompt
    if !args.force {
        print!("Delete {} file(s)? [y/N] ", files.len());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        if input != "y" && input != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Compute thresholds for safe mode
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let atime_threshold = args.older_than.map(|days| now - (days as i64 * 86400));
    let max_size_bytes = args
        .max_size
        .as_deref()
        .map(parse_size)
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?;

    // Delete files in parallel
    let deleted = AtomicU64::new(0);
    let skipped = AtomicU64::new(0);
    let failed = AtomicU64::new(0);
    let missing = AtomicU64::new(0);

    let safe = args.safe;
    let verbose = args.verbose;

    files.par_iter().for_each(|file| {
        let path = std::path::Path::new(&file.path);

        // Safe mode: re-stat the file and verify conditions
        if safe {
            match fs::metadata(path) {
                Ok(meta) => {
                    // Check atime if --older-than was specified
                    if let Some(threshold) = atime_threshold {
                        let current_atime = meta.atime();
                        if current_atime >= threshold {
                            if verbose {
                                println!("SKIP (accessed since index): {}", file.path);
                            }
                            skipped.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                    }

                    // Check size if --max-size was specified
                    if let Some(max_size) = max_size_bytes {
                        let current_size = meta.len() as i64;
                        if current_size > max_size {
                            if verbose {
                                println!("SKIP (size changed): {}", file.path);
                            }
                            skipped.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    if verbose {
                        println!("SKIP (not found): {}", file.path);
                    }
                    missing.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                Err(e) => {
                    if verbose {
                        eprintln!("FAIL (stat error): {}: {}", file.path, e);
                    }
                    failed.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
        }

        // Attempt deletion
        match fs::remove_file(path) {
            Ok(()) => {
                if verbose {
                    println!("DELETE: {}", file.path);
                }
                deleted.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if verbose {
                    println!("SKIP (not found): {}", file.path);
                }
                missing.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                if verbose {
                    eprintln!("FAIL: {}: {}", file.path, e);
                }
                failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Extract final counts
    let deleted = deleted.load(Ordering::Relaxed);
    let skipped = skipped.load(Ordering::Relaxed);
    let failed = failed.load(Ordering::Relaxed);
    let missing = missing.load(Ordering::Relaxed);

    // Print summary
    println!("\nDeleted: {}", deleted);
    if missing > 0 {
        println!("Missing: {}", missing);
    }
    if skipped > 0 {
        println!("Skipped (safe mode): {}", skipped);
    }
    if failed > 0 {
        println!("Failed: {}", failed);
    }

    Ok(())
}
