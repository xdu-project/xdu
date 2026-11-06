//! CLI argument definitions for all xdu binaries.
//!
//! These are extracted into a shared module so that `gen-completions` can
//! access the `clap::Command` objects via `CommandFactory::command()`.

use std::path::PathBuf;

use clap::{Args, Parser};

/// Arguments for `xdu` — the filesystem indexer.
#[derive(Parser, Debug)]
#[command(
    name = "xdu",
    version,
    about = "Build a distributed file metadata index in Parquet format",
    after_help = "\
Examples:
  xdu /data/scratch -o /index/scratch -j 8
  xdu /data/scratch -o /index/scratch --partition alice,bob
  xdu /data/scratch -o /index/scratch --apparent-size
  xdu /data/scratch -o /index/scratch --block-size 128K
  xdu /data/scratch -o s3://indexes/scratch"
)]
pub struct XduArgs {
    /// Top-level directory to index
    #[arg(value_name = "DIR")]
    pub dir: PathBuf,

    /// Output directory for the Parquet index, or s3://BUCKET/PREFIX to build it into S3
    #[arg(short, long, value_name = "DIR")]
    pub outdir: PathBuf,

    /// Number of parallel threads
    #[arg(short, long, default_value = "4", env = "XDU_JOBS")]
    pub jobs: usize,

    /// Number of records per output chunk
    #[arg(short = 'B', long, default_value = "100000")]
    pub buffsize: usize,

    /// Report apparent sizes (file length) rather than disk usage.
    /// By default, xdu reports actual disk usage from st_blocks.
    #[arg(long)]
    pub apparent_size: bool,

    /// Round sizes up to this block size (e.g., 128K, 1M).
    /// Useful when st_blocks is inaccurate (e.g., over NFS) and you know the filesystem block size.
    /// Implies --apparent-size for the base size, then rounds up.
    #[arg(short = 'k', long, value_name = "SIZE")]
    pub block_size: Option<String>,

    /// Index only specific partitions (top-level subdirectory names, comma-separated).
    #[arg(short, long, value_name = "NAMES", value_delimiter = ',')]
    pub partition: Option<Vec<String>>,

    /// Continue past unreadable files or directories instead of failing the run.
    /// By default an unreadable region (permission or I/O error) makes xdu exit
    /// non-zero so a silently-incomplete index is never mistaken for a complete one;
    /// with this flag those errors are downgraded to warnings and the crawl indexes
    /// every reachable file and exits 0. Vanished-file races are always tolerated.
    #[arg(long)]
    pub allow_errors: bool,
}

/// Arguments for `xdu-find` — the index query tool.
#[derive(Parser, Debug)]
#[command(
    name = "xdu-find",
    version,
    about = "Query a file metadata index for matching paths",
    after_help = "Examples:
  xdu-find -i /index/scratch -p '*.py' --min-size 1M
  xdu-find -i /index/scratch --older-than 90 -f size
  xdu-find -i /index/scratch -u alice --count
  xdu-find -i /index/scratch --regex -p '\\.tmp$' --older-than 30 --count
  xdu-find -i /index/scratch --top 10"
)]
pub struct XduFindArgs {
    /// Path to the Parquet index directory
    #[arg(short, long, value_name = "DIR", env = "XDU_INDEX")]
    pub index: PathBuf,

    /// Glob pattern to match paths (e.g., '*.py'); see --regex for regular expressions
    #[arg(short, long, value_name = "PATTERN")]
    pub pattern: Option<String>,

    /// Interpret --pattern as a regular expression instead of a glob
    #[arg(long)]
    pub regex: bool,

    /// Filter by partition (user directory name)
    #[arg(short = 'u', long, value_name = "NAME")]
    pub partition: Option<String>,

    /// Minimum file size (e.g., 1K, 10M, 1G)
    #[arg(long, value_name = "SIZE")]
    pub min_size: Option<String>,

    /// Maximum file size (e.g., 1K, 10M, 1G)
    #[arg(long, value_name = "SIZE")]
    pub max_size: Option<String>,

    /// Files not accessed in N days
    #[arg(long, value_name = "DAYS")]
    pub older_than: Option<u64>,

    /// Files accessed within N days
    #[arg(long, value_name = "DAYS")]
    pub newer_than: Option<u64>,

    /// Files owned by this user (name or numeric uid)
    #[arg(long, value_name = "USER")]
    pub owner: Option<String>,

    /// Files owned by this group (name or numeric gid)
    #[arg(long, value_name = "GROUP")]
    pub group: Option<String>,

    /// Permission filter: bare octal matches exactly (644), /OCTAL matches when
    /// any listed bit is set (/002), &OCTAL when all are (&4000)
    #[arg(long, value_name = "SPEC")]
    pub mode: Option<String>,

    /// Files not modified in N days
    #[arg(long, value_name = "DAYS")]
    pub mtime_older_than: Option<u64>,

    /// Files modified within N days
    #[arg(long, value_name = "DAYS")]
    pub mtime_newer_than: Option<u64>,

    /// Output format: path (default), size, atime, csv, json
    #[arg(short, long, default_value = "path")]
    pub format: String,

    /// Limit number of results
    #[arg(short, long)]
    pub limit: Option<usize>,

    /// Count matching records instead of listing them
    #[arg(short, long)]
    pub count: bool,

    /// Show top N partitions by file count (for identifying large partitions)
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
}

/// Arguments for `xdu-view` — the interactive TUI explorer.
#[derive(Parser, Debug)]
#[command(
    name = "xdu-view",
    version,
    about = "Interactive TUI for exploring a file metadata index",
    after_help = "\
Examples:
  xdu-view -i /index/scratch
  xdu-view -i /index/scratch -u alice
  xdu-view -i /index/scratch --older-than 90 --sort size"
)]
pub struct XduViewArgs {
    /// Path to the Parquet index directory
    #[arg(short, long, value_name = "DIR", env = "XDU_INDEX")]
    pub index: PathBuf,

    /// Initial partition to view (optional, shows partition list if omitted)
    #[arg(short = 'u', long, value_name = "NAME")]
    pub partition: Option<String>,

    /// Glob pattern to match paths (e.g., '*.py'); see --regex for regular expressions
    #[arg(short, long, value_name = "PATTERN")]
    pub pattern: Option<String>,

    /// Interpret --pattern as a regular expression instead of a glob
    #[arg(long)]
    pub regex: bool,

    /// Minimum file size (e.g., 1K, 10M, 1G)
    #[arg(long, value_name = "SIZE")]
    pub min_size: Option<String>,

    /// Maximum file size (e.g., 1K, 10M, 1G)
    #[arg(long, value_name = "SIZE")]
    pub max_size: Option<String>,

    /// Files not accessed in N days
    #[arg(long, value_name = "DAYS")]
    pub older_than: Option<u64>,

    /// Files accessed within N days
    #[arg(long, value_name = "DAYS")]
    pub newer_than: Option<u64>,

    /// Sort order: name, size-asc, size-desc, count-asc, count-desc, age-asc, age-desc
    #[arg(short, long, default_value = "name")]
    pub sort: String,
}

/// Arguments for `xdu-rm` — the bulk deletion tool.
#[derive(Parser, Debug)]
#[command(
    name = "xdu-rm",
    version,
    about = "Delete files matching index query criteria",
    after_help = "Examples:
  xdu-rm -i /index/scratch -u alice --older-than 180 -n
  xdu-rm -i /index/scratch -p '*.tmp' --force
  xdu-rm -i /index/scratch --min-size 1G --safe"
)]
pub struct XduRmArgs {
    /// Path to the Parquet index directory
    #[arg(short, long, value_name = "DIR", env = "XDU_INDEX")]
    pub index: PathBuf,

    /// Glob pattern to match paths (e.g., '*.py'); see --regex for regular expressions
    #[arg(short, long, value_name = "PATTERN")]
    pub pattern: Option<String>,

    /// Interpret --pattern as a regular expression instead of a glob
    #[arg(long)]
    pub regex: bool,

    /// Filter by partition (user directory name)
    #[arg(short = 'u', long, value_name = "NAME")]
    pub partition: Option<String>,

    /// Minimum file size (e.g., 1K, 10M, 1G)
    #[arg(long, value_name = "SIZE")]
    pub min_size: Option<String>,

    /// Maximum file size (e.g., 1K, 10M, 1G)
    #[arg(long, value_name = "SIZE")]
    pub max_size: Option<String>,

    /// Files not accessed in N days
    #[arg(long, value_name = "DAYS")]
    pub older_than: Option<u64>,

    /// Files accessed within N days
    #[arg(long, value_name = "DAYS")]
    pub newer_than: Option<u64>,

    /// Limit number of files to delete
    #[arg(short, long)]
    pub limit: Option<usize>,

    /// Show what would be deleted without deleting
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Verify file metadata before deletion (re-stat to check atime/size)
    #[arg(long)]
    pub safe: bool,

    /// Skip confirmation prompt
    #[arg(short, long)]
    pub force: bool,

    /// Show detailed output for each file
    #[arg(short, long)]
    pub verbose: bool,

    /// Number of parallel threads for deletion
    #[arg(short, long, default_value = "4", env = "XDU_JOBS")]
    pub jobs: usize,
}

/// The selection every select-then-act tool shares: `xdu-find`'s filters, in its order and
/// wording, then `xdu-rm`'s action flags.
///
/// Flattened into each tool's own arguments, so the flags are declared once and clap renders
/// them exactly as if written inline. `-f` is `--force` here, as in `xdu-rm`, not `xdu-find`'s
/// `--format`.
#[derive(Args, Debug, Clone)]
pub struct SelectArgs {
    /// Path to the Parquet index directory
    #[arg(short, long, value_name = "DIR", env = "XDU_INDEX")]
    pub index: PathBuf,

    /// Glob pattern to match paths (e.g., '*.py'); see --regex for regular expressions
    #[arg(short, long, value_name = "PATTERN")]
    pub pattern: Option<String>,

    /// Interpret --pattern as a regular expression instead of a glob
    #[arg(long)]
    pub regex: bool,

    /// Filter by partition (user directory name)
    #[arg(short = 'u', long, value_name = "NAME")]
    pub partition: Option<String>,

    /// Minimum file size (e.g., 1K, 10M, 1G)
    #[arg(long, value_name = "SIZE")]
    pub min_size: Option<String>,

    /// Maximum file size (e.g., 1K, 10M, 1G)
    #[arg(long, value_name = "SIZE")]
    pub max_size: Option<String>,

    /// Files not accessed in N days
    #[arg(long, value_name = "DAYS")]
    pub older_than: Option<u64>,

    /// Files accessed within N days
    #[arg(long, value_name = "DAYS")]
    pub newer_than: Option<u64>,

    /// Files owned by this user (name or numeric uid)
    #[arg(long, value_name = "USER")]
    pub owner: Option<String>,

    /// Files owned by this group (name or numeric gid)
    #[arg(long, value_name = "GROUP")]
    pub group: Option<String>,

    /// Permission filter: bare octal matches exactly (644), /OCTAL matches when
    /// any listed bit is set (/002), &OCTAL when all are (&4000)
    #[arg(long, value_name = "SPEC")]
    pub mode: Option<String>,

    /// Files not modified in N days
    #[arg(long, value_name = "DAYS")]
    pub mtime_older_than: Option<u64>,

    /// Files modified within N days
    #[arg(long, value_name = "DAYS")]
    pub mtime_newer_than: Option<u64>,

    /// Act on at most N files, the N whose paths sort first
    #[arg(short, long)]
    pub limit: Option<usize>,

    /// Show each source and its destination without writing anything
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Re-check every given filter against each file's live metadata before acting on it
    #[arg(long)]
    pub safe: bool,

    /// Skip confirmation prompt
    #[arg(short, long)]
    pub force: bool,

    /// List each file and its outcome
    #[arg(short, long)]
    pub verbose: bool,

    /// Number of parallel threads
    #[arg(short, long, default_value = "4", env = "XDU_JOBS")]
    pub jobs: usize,
}

/// Arguments for `xdu-cp` — the bulk copy tool.
#[derive(Parser, Debug)]
#[command(
    name = "xdu-cp",
    version,
    about = "Copy files matching index query criteria into a directory tree",
    after_help = "Examples:
  xdu-cp -i /index/scratch -u alice --older-than 365 --into /stage --root /scratch -n
  xdu-cp -i /index/scratch -p '*.log' --into /stage --root /scratch --force
  xdu-cp -i /index/scratch --min-size 1G --into /stage -v"
)]
pub struct XduCpArgs {
    #[command(flatten)]
    pub select: SelectArgs,

    /// Existing directory to copy into; each file lands at DIR/<its path relative to --root>
    #[arg(long, value_name = "DIR")]
    pub into: PathBuf,

    /// Prefix stripped from each indexed path to form its place under --into
    #[arg(long, value_name = "DIR", default_value = "/")]
    pub root: PathBuf,
}
