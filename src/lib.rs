//! Shared types and utilities for xdu tools.

pub mod cli;
pub mod crawl;
pub mod engine;
pub mod s3;
pub mod view_query;

pub use s3::{IndexLocator, S3Locator, S3Settings};

use std::ffi::CString;
use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use arrow::datatypes::{DataType, Field, Schema};

/// Partition name reserved for files lying directly in the indexed root.
///
/// Part of the on-disk layout contract rather than of the crawl, so the writer and every
/// reader name it from one place: two copies of this string could drift apart and quietly
/// break the depth-1 partition.
pub const ROOT_PARTITION: &str = "__root__";

/// Run-level completion marker written at the index root when a crawl succeeds.
///
/// A dotfile, so the readers' `*/*.parquet` glob never mistakes it for a partition. The
/// reverse direction — a partition directory landing *on* the marker path — is guarded by
/// this name's membership in `RESERVED_INDEX_NAMES`.
pub const COMPLETION_MARKER: &str = ".xdu-complete";

/// On-disk index format version recorded in every completion marker.
///
/// 2 names the current layout: eight-column `get_schema()` rows under
/// `<partition>/NNNNNN.parquet` plus this marker. A future layout or schema change bumps
/// this, and readers refuse what they do not understand instead of misreading it — the
/// escape hatch the schema-stability invariant requires before any column is added.
pub const INDEX_FORMAT_VERSION: u32 = 2;

/// Every name the index root already claims, paired with what claims it.
///
/// `<index>/` holds exactly two kinds of entry: partition directories named after the
/// source tree's top-level subdirectories, and the reserved `COMPLETION_MARKER` dotfile.
/// A top-level source directory whose name is already claimed would be written *as* that
/// entry — clobbering the synthetic loose-file partition's chunk ids, or occupying the
/// marker path so the run cannot attest itself and no later run can clear it. So
/// `crawl::build_work_queue` rejects one, naming what the collision is with.
///
/// The guard iterates this list rather than testing names one by one, so reserving a new
/// name at the index root extends the rejection by construction: the collision class stays
/// closed instead of depending on someone remembering the second half of it.
pub const RESERVED_INDEX_NAMES: &[(&str, &str)] = &[
    (
        ROOT_PARTITION,
        "the partition holding loose top-level files",
    ),
    (COMPLETION_MARKER, "the run-completion marker"),
];

/// Most a reader will ever read from a completion marker. The writer emits ~100 bytes; the
/// cap exists so a reader cannot be made to pull an arbitrarily large file into memory by
/// whatever left a file of that name in a group-writable index directory, or an object of
/// that key in a bucket.
pub const MARKER_READ_LIMIT: u64 = 64 * 1024;

/// The tolerated-error count a completion marker records, if its body states one.
///
/// `None` for a missing or unparseable `errors=` key. An unrecognized marker body says
/// nothing about errors, so a reader stays exactly as quiet as it was before the marker
/// existed rather than inventing a warning out of a format it does not understand. The
/// first key trimming to `errors` decides the answer.
pub fn completion_marker_errors(body: &str) -> Option<u64> {
    for line in body.lines() {
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "errors"
        {
            return value.trim().parse().ok();
        }
    }
    None
}

/// What a guarded read of the completion marker found.
///
/// The marker lives in a directory operators share, so whatever sits at that path is
/// untrusted: a FIFO or device node would block a reader that opened it blindly, and an
/// enormous file would be pulled into memory. One `stat` decides all three cases before
/// anything is opened. Over S3 the size comes from a `HEAD` instead, and an object over the
/// cap is never fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerRead {
    /// No entry at the marker path: an interrupted run, or an index predating the marker.
    Absent,
    /// An entry that must not be read: not a regular file, over `MARKER_READ_LIMIT`, or
    /// unreadable since the stat. Attests nothing, and is never opened.
    Unreadable,
    /// An S3 marker object of this many bytes, over `MARKER_READ_LIMIT`. Attests nothing,
    /// and was never fetched.
    Oversized(u64),
    /// A regular file of sane size, read — possibly as empty when it vanished mid-read.
    Body(String),
}

/// Read the completion marker without trusting what is at its path. Both the
/// completeness warning and the version gate go through here, so the non-blocking and
/// size-cap protections stay single-sourced.
fn read_completion_marker(index: &Path) -> MarkerRead {
    let marker = index.join(COMPLETION_MARKER);

    // One `stat`, not two: `Path::exists()` *is* `metadata().is_ok()`, so an absent marker
    // behaves exactly as it did before bodies were read at all.
    let meta = match fs::metadata(&marker) {
        Ok(meta) => meta,
        Err(_) => return MarkerRead::Absent,
    };

    // Presence alone already attests that the run finished; the body only ever adds detail.
    // So consulting it must never cost more than not consulting it: opening a FIFO, socket
    // or device node of this name would block the reader forever, and an oversized file
    // would be pulled into memory. Neither is worth a detail, so neither is opened.
    if !meta.is_file() || meta.len() > MARKER_READ_LIMIT {
        return MarkerRead::Unreadable;
    }

    // Anything unreadable here — a permission change, non-UTF-8 bytes, or the marker being
    // deleted since the stat above — degrades to an empty body, which callers treat as
    // unattested rather than as evidence of anything.
    MarkerRead::Body(fs::read_to_string(&marker).unwrap_or_default())
}

/// The index format version a completion marker body states, if it states one.
///
/// Mirrors `completion_marker_errors`: the first key trimming to `format` decides, and
/// anything absent or unparseable is `None` rather than a guess. The `xdu=` key beside
/// it names the tool release that wrote the index, which outpaces format changes and so
/// is never a compatibility answer.
pub fn completion_marker_format(body: &str) -> Option<u32> {
    for line in body.lines() {
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "format"
        {
            return value.trim().parse().ok();
        }
    }
    None
}

/// The part of an index glob below the index root: `<partition>/*.parquet`, with `*` for
/// every partition. Both transports share it, so the layout is written once.
fn index_glob_tail(partition: Option<&str>) -> String {
    format!("{}/*.parquet", partition.unwrap_or("*"))
}

/// The `read_parquet` glob for an index, optionally scoped to a single partition.
///
/// Every local reader goes through here, so the index layout (`<index>/<partition>/*.parquet`)
/// is expressed once. It is also the single seam where local index paths and partition names
/// reach SQL, which is where escaping belongs when it is added.
pub fn index_glob(index: &Path, partition: Option<&str>) -> String {
    format!("{}/{}", index.display(), index_glob_tail(partition))
}

/// The `read_parquet` glob for an index under an S3 prefix, optionally scoped to a single
/// partition: `s3://<bucket>/<prefix>/<partition>/*.parquet`.
///
/// It reaches SQL only as a bound parameter (`parquet_source`), never as text.
pub fn index_glob_s3(index: &S3Locator, partition: Option<&str>) -> String {
    format!("{index}/{}", index_glob_tail(partition))
}

/// The `read_parquet(...)` call a reader puts in its `FROM` clause, and the values it binds.
///
/// For a local index the glob is written into the text as it always was, and nothing is
/// bound. For an S3 index the text is `read_parquet(?, …)` and the glob, which carries the
/// bucket, the prefix and any partition name, is the one bound value. `filename` adds the
/// `filename=true` column a per-partition query groups by.
///
/// Both arms turn DuckDB's Hive detection off. The layout's directory names are bare, but
/// nothing stops one from reading `key=value`: a partition named `size=0`, or an index stored
/// under `…/mtime=0/`. When every globbed path carries such a name, detection adds a column
/// `key` holding `value` that shadows the row's own, so every row reports that value and every
/// filter, sum and sort on the column reads it. The option is a constant: it puts no value in
/// the text that was not there before.
pub fn parquet_source(
    index: &IndexLocator,
    partition: Option<&str>,
    filename: bool,
) -> (String, Vec<String>) {
    let option = if filename { ", filename=true" } else { "" };
    match index {
        IndexLocator::Local(path) => (
            format!(
                "read_parquet('{}', hive_partitioning = false{option})",
                index_glob(path, partition)
            ),
            Vec::new(),
        ),
        IndexLocator::S3(locator) => (
            format!("read_parquet(?, hive_partitioning = false{option})"),
            vec![index_glob_s3(locator, partition)],
        ),
    }
}

/// Why an S3 request failed, as far as the reader can tell from DuckDB's error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3Cause {
    /// The endpoint's TLS certificate does not chain to a CA in the trust source.
    Untrusted,
    /// No connection to the endpoint could be established.
    Unreachable,
    /// The endpoint refused the request's credentials or signature (HTTP 403).
    Forbidden,
    /// The endpoint answered 404: no such bucket for a listing, no such key for an object.
    NotFound,
    /// Anything else, carrying DuckDB's text unchanged.
    Other(String),
}

/// Name the cause of a failed S3 request from DuckDB's error text.
///
/// `httpfs` reports causes only in prose, so this matches the texts it is known to produce: a
/// refused certificate is httplib's `SSL server verification failed` (or curl's `SSL peer
/// certificate`, should anything select curl); a dead endpoint is `Could not establish
/// connection`; a rejected signature carries `HTTP 403`,
/// or `403 (Forbidden)` in the form an object request takes; and a missing bucket or key
/// carries `HTTP 404` or `404 (Not Found)` (a missing key's text says `Unable to connect`,
/// though the endpoint answered). The fallback keeps DuckDB's text, which names the URL but
/// never a credential.
pub fn classify_s3_error(error: &duckdb::Error) -> S3Cause {
    classify_s3_text(&error.to_string())
}

fn classify_s3_text(text: &str) -> S3Cause {
    if text.contains("SSL server verification failed") || text.contains("SSL peer certificate") {
        S3Cause::Untrusted
    } else if text.contains("Could not establish connection") {
        S3Cause::Unreachable
    } else if text.contains("HTTP 403") || text.contains("403 (Forbidden)") {
        S3Cause::Forbidden
    } else if text.contains("HTTP 404") || text.contains("404 (Not Found)") {
        S3Cause::NotFound
    } else {
        S3Cause::Other(text.to_string())
    }
}

/// The error for a failed S3 request that is not a missing object, naming the cause.
fn s3_request_error(index: &S3Locator, what: &str, error: &duckdb::Error) -> anyhow::Error {
    match classify_s3_error(error) {
        S3Cause::Untrusted => anyhow::anyhow!(
            "the S3 endpoint's TLS certificate is not trusted while {what} for {index}; it must \
             chain to a CA in the file AWS_CA_BUNDLE names, else the platform's CA bundle \
             ({error})"
        ),
        S3Cause::Unreachable => anyhow::anyhow!(
            "cannot reach the S3 endpoint while {what} for {index}: no connection could be \
             established; check AWS_ENDPOINT_URL_S3, AWS_ENDPOINT_URL or the profile's \
             endpoint, and the network"
        ),
        S3Cause::Forbidden => anyhow::anyhow!(
            "the S3 endpoint refused the credentials while {what} for {index} (HTTP 403); \
             check the access key, secret and region"
        ),
        S3Cause::NotFound => anyhow::anyhow!(
            "bucket '{}' does not exist at the S3 endpoint (HTTP 404 while {what} for {index})",
            index.bucket()
        ),
        S3Cause::Other(text) => {
            anyhow::anyhow!("S3 request failed while {what} for {index}: {text}")
        }
    }
}

/// Read an S3 index's completion marker without fetching more than `MARKER_READ_LIMIT`.
///
/// Three statements, each a step that can fail on its own terms. A listing of everything under
/// the prefix proves the endpoint is reachable with accepted credentials and that the bucket
/// exists, and an empty one means no index is there at all; both are errors naming the cause.
/// Then the marker's size, which costs a `HEAD` and no `GET`: a missing key is `Absent`, and a
/// size over the cap is `Oversized` without the object being fetched. Only then the content,
/// read whole in one ranged `GET`. Bytes that are not UTF-8 read as an empty body, as they do
/// for a local marker.
pub fn read_completion_marker_s3(
    conn: &duckdb::Connection,
    index: &S3Locator,
) -> anyhow::Result<MarkerRead> {
    let listed: i64 = conn
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM glob(?) LIMIT 1)",
            [index.object_url("**")],
            |row| row.get(0),
        )
        .map_err(|e| s3_request_error(index, "listing the index", &e))?;
    if listed == 0 {
        anyhow::bail!("no index at {index}: the prefix holds no objects");
    }

    let marker = index.object_url(COMPLETION_MARKER);
    let size: i64 =
        match conn.query_row("SELECT size FROM read_blob(?)", [&marker], |row| row.get(0)) {
            Ok(size) => size,
            Err(e) => {
                return match classify_s3_error(&e) {
                    S3Cause::NotFound => Ok(MarkerRead::Absent),
                    _ => Err(s3_request_error(
                        index,
                        "reading the completion marker's size",
                        &e,
                    )),
                };
            }
        };
    let size = u64::try_from(size).unwrap_or(0);
    if size > MARKER_READ_LIMIT {
        return Ok(MarkerRead::Oversized(size));
    }

    let content: Vec<u8> = conn
        .query_row("SELECT content FROM read_blob(?)", [&marker], |row| {
            row.get(0)
        })
        .map_err(|e| s3_request_error(index, "reading the completion marker", &e))?;
    Ok(MarkerRead::Body(
        String::from_utf8(content).unwrap_or_default(),
    ))
}

/// A warning to print when an index cannot be trusted to be complete, else `None`.
///
/// Two ways an index falls short. It may carry **no marker**: the crawler writes one only
/// when a run finishes, so absence means a failed or interrupted run — or an index that
/// predates the marker entirely. Or it may carry a marker that records **tolerated errors**:
/// an `xdu --allow-errors` run finishes and is marked complete, yet knowingly skipped
/// whatever it could not read. That second case matters most to `xdu-rm`, whose risk is
/// precisely the files an index does not know about, and the operator running it weeks later
/// never gave the consent the build-time `--allow-errors` expressed.
///
/// Readers warn and carry on rather than refusing: every index built before the marker
/// existed is still perfectly queryable, and breaking those would be worse than the risk
/// being flagged.
pub fn index_completion_warning(index: &Path) -> Option<String> {
    marker_completion_warning(&read_completion_marker(index), &index.display().to_string())
}

/// `index_completion_warning` over a marker already read, for the index named `index`.
///
/// The S3 readers read the marker once, since each read is a round trip, and pass the
/// result to both gates.
pub fn marker_completion_warning(marker: &MarkerRead, index: &str) -> Option<String> {
    let body = match marker {
        MarkerRead::Absent => {
            return Some(format!(
                "warning: {} has no completion marker ({}); it may be from an interrupted \
                 run or predate the marker, so results may be incomplete",
                index, COMPLETION_MARKER
            ));
        }
        // An entry that cannot be read offers no detail, so there is nothing to add —
        // which is the behavior a marker-present index had before bodies were read.
        MarkerRead::Unreadable | MarkerRead::Oversized(_) => return None,
        MarkerRead::Body(body) => body,
    };

    match completion_marker_errors(body) {
        Some(errors) if errors > 0 => Some(format!(
            "warning: {} was indexed with {} tolerated error(s) (xdu --allow-errors); the \
             affected paths were skipped, so results may be incomplete",
            index, errors
        )),
        _ => None,
    }
}

/// A refusal to read an index whose format version is unknown or unsupported, else `None`.
///
/// Every index built before versioning carries no `format=` key, so absence of a version
/// is not grandfathered: those rows predate the only check that could vouch for their
/// layout, and reading them blind is the silent misread this exists to prevent. The
/// remedy is always a re-crawl, which is why the diagnostic says so.
pub fn index_version_error(index: &Path) -> Option<String> {
    marker_version_error(&read_completion_marker(index), &index.display().to_string())
}

/// `index_version_error` over a marker already read, for the index named `index`.
pub fn marker_version_error(marker: &MarkerRead, index: &str) -> Option<String> {
    let found = match marker {
        MarkerRead::Body(body) => completion_marker_format(body),
        MarkerRead::Oversized(size) => {
            return Some(format!(
                "error: {} has a completion marker of {} bytes, over the {}-byte limit a \
                 reader reads; it was not fetched, so the index format version is unknown",
                index, size, MARKER_READ_LIMIT
            ));
        }
        // Absent or unreadable attestation answers nothing about the layout.
        MarkerRead::Absent | MarkerRead::Unreadable => None,
    };

    match found {
        Some(version) if version == INDEX_FORMAT_VERSION => None,
        Some(version) => Some(format!(
            "error: {} has index format version {}, but this xdu supports version {}; \
             re-index with this xdu to query it",
            index, version, INDEX_FORMAT_VERSION
        )),
        None => Some(format!(
            "error: {} carries no index format version (it predates index versioning or \
             comes from an interrupted run); re-index with this xdu to query it",
            index,
        )),
    }
}

/// Round size up to the nearest block boundary.
pub fn round_to_block(size: u64, block_size: u64) -> u64 {
    if block_size == 0 || size == 0 {
        return size;
    }
    size.div_ceil(block_size) * block_size
}

/// Determines how to calculate file size
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizeMode {
    /// Use st_blocks * 512 (actual disk usage)
    DiskUsage,
    /// Use st_size (apparent/logical size)
    ApparentSize,
    /// Use st_size rounded up to block size
    BlockRounded(u64),
}

impl SizeMode {
    /// Calculate the size based on the mode.
    /// For DiskUsage, provide (blocks * 512, file_len).
    /// For ApparentSize and BlockRounded, only file_len is used.
    pub fn calculate(&self, disk_usage: u64, file_len: u64) -> u64 {
        match self {
            SizeMode::DiskUsage => disk_usage,
            SizeMode::ApparentSize => file_len,
            SizeMode::BlockRounded(block_size) => round_to_block(file_len, *block_size),
        }
    }
}

/// Parse a human-readable size string into bytes.
/// Supports suffixes: K, M, G, T (and KiB, MiB, GiB, TiB variants).
pub fn parse_size(s: &str) -> Result<i64, String> {
    let s = s.trim().to_uppercase();
    let (num, mult) = if let Some(n) = s.strip_suffix("TIB") {
        (n, 1024_i64 * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("T") {
        (n, 1024_i64 * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("GIB") {
        (n, 1024_i64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("G") {
        (n, 1024_i64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("MIB") {
        (n, 1024_i64 * 1024)
    } else if let Some(n) = s.strip_suffix("M") {
        (n, 1024_i64 * 1024)
    } else if let Some(n) = s.strip_suffix("KIB") {
        (n, 1024_i64)
    } else if let Some(n) = s.strip_suffix("K") {
        (n, 1024_i64)
    } else if let Some(n) = s.strip_suffix("B") {
        (n, 1)
    } else {
        (s.as_str(), 1)
    };

    let num: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("Invalid size: {}", s))?;
    Ok((num * mult as f64) as i64)
}

/// Sort mode for directory listings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortMode {
    /// Alphabetical by name (directories first)
    #[default]
    Name,
    /// By total size, descending
    SizeDesc,
    /// By total size, ascending
    SizeAsc,
    /// By file count, descending
    CountDesc,
    /// By file count, ascending
    CountAsc,
    /// By age (oldest first - least recent access)
    AgeDesc,
    /// By age (newest first - most recent access)
    AgeAsc,
}

impl SortMode {
    /// All sort modes in display order.
    pub const ALL: [SortMode; 7] = [
        SortMode::Name,
        SortMode::SizeDesc,
        SortMode::SizeAsc,
        SortMode::CountDesc,
        SortMode::CountAsc,
        SortMode::AgeDesc,
        SortMode::AgeAsc,
    ];

    /// Returns the next sort mode in the cycle.
    pub fn next(self) -> Self {
        match self {
            SortMode::Name => SortMode::SizeDesc,
            SortMode::SizeDesc => SortMode::SizeAsc,
            SortMode::SizeAsc => SortMode::CountDesc,
            SortMode::CountDesc => SortMode::CountAsc,
            SortMode::CountAsc => SortMode::AgeDesc,
            SortMode::AgeDesc => SortMode::AgeAsc,
            SortMode::AgeAsc => SortMode::Name,
        }
    }

    /// Returns the previous sort mode in the cycle.
    pub fn prev(self) -> Self {
        match self {
            SortMode::Name => SortMode::AgeAsc,
            SortMode::SizeDesc => SortMode::Name,
            SortMode::SizeAsc => SortMode::SizeDesc,
            SortMode::CountDesc => SortMode::SizeAsc,
            SortMode::CountAsc => SortMode::CountDesc,
            SortMode::AgeDesc => SortMode::CountAsc,
            SortMode::AgeAsc => SortMode::AgeDesc,
        }
    }

    /// Returns the SQL ORDER BY clause for this sort mode.
    /// When sorting by Name, directories are grouped first.
    pub fn to_order_by(&self, dirs_first: bool) -> &'static str {
        match self {
            SortMode::Name if dirs_first => "bool_or(is_dir) DESC, component",
            SortMode::Name => "component",
            SortMode::SizeDesc => "total_size DESC",
            SortMode::SizeAsc => "total_size ASC",
            SortMode::CountDesc => "file_count DESC",
            SortMode::CountAsc => "file_count ASC",
            SortMode::AgeDesc => "latest_atime ASC", // oldest first = smallest atime
            SortMode::AgeAsc => "latest_atime DESC", // newest first = largest atime
        }
    }

    /// Returns the ORDER BY clause for partition listing.
    pub fn to_partition_order_by(&self) -> &'static str {
        match self {
            SortMode::Name => "partition",
            SortMode::SizeDesc => "total_size DESC",
            SortMode::SizeAsc => "total_size ASC",
            SortMode::CountDesc => "file_count DESC",
            SortMode::CountAsc => "file_count ASC",
            SortMode::AgeDesc => "latest_atime ASC", // oldest first
            SortMode::AgeAsc => "latest_atime DESC", // newest first
        }
    }
}

impl fmt::Display for SortMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SortMode::Name => write!(f, "name"),
            SortMode::SizeDesc => write!(f, "size-desc"),
            SortMode::SizeAsc => write!(f, "size-asc"),
            SortMode::CountDesc => write!(f, "count-desc"),
            SortMode::CountAsc => write!(f, "count-asc"),
            SortMode::AgeDesc => write!(f, "age-desc"),
            SortMode::AgeAsc => write!(f, "age-asc"),
        }
    }
}

impl FromStr for SortMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "name" => Ok(SortMode::Name),
            "size-desc" | "size" => Ok(SortMode::SizeDesc),
            "size-asc" => Ok(SortMode::SizeAsc),
            "count-desc" | "count" => Ok(SortMode::CountDesc),
            "count-asc" => Ok(SortMode::CountAsc),
            "age-desc" | "age" | "oldest" => Ok(SortMode::AgeDesc),
            "age-asc" | "newest" => Ok(SortMode::AgeAsc),
            _ => Err(format!(
                "Invalid sort mode: {}. Use: name, size-desc, size-asc, count-desc, count-asc, age-desc, age-asc",
                s
            )),
        }
    }
}

/// Translate a glob pattern into an anchored regular expression for `regexp_matches`.
///
/// The query builder speaks one dialect — regex — so a glob is converted at ingestion and
/// the database never learns a second one. `*` crosses `/`: a glob names the whole path,
/// and `*.py` must match at any depth rather than only directly below the indexed root.
/// `?` matches a single character, `[...]` is a character class (`[!...]` negates it,
/// `[a-z]` is a range), and `\` quotes the next character literally. Every other regex
/// metacharacter in a literal position is escaped, so the translation cannot smuggle in
/// regex syntax the user did not write. A `-` first or last in a class is literal while a
/// middle one opens a range — the regex grammar agrees on all three positions, so it
/// publishes unescaped — and a descending range is rejected here, so every glob failure
/// surfaces at ingestion rather than at the database. Output is wrapped `^(?:…)$` because
/// `regexp_matches` is an unanchored substring search while a glob matches the entire path.
pub fn glob_to_regex(glob: &str) -> Result<String, String> {
    if glob.is_empty() {
        return Err("Empty glob pattern matches no path".to_string());
    }
    let mut out = String::from("^(?:");
    let mut chars = glob.char_indices().peekable();
    while let Some((pos, c)) = chars.next() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '\\' => match chars.next() {
                Some((_, quoted)) => push_regex_literal(&mut out, quoted),
                None => {
                    return Err(format!(
                        "Trailing backslash at byte {pos} in glob pattern: {glob}"
                    ));
                }
            },
            '[' => {
                let mut class = String::new();
                if matches!(chars.peek(), Some((_, '!'))) {
                    chars.next();
                    class.push('^');
                }
                // Last literal endpoint pushed, for range validation. None at class
                // start — the `^` negation marks nothing — and after a range
                // operator, whose endpoint cannot open another range.
                let mut prev: Option<char> = None;
                let mut closed = false;
                while let Some((_, inner)) = chars.next() {
                    match inner {
                        // A `]` in first position is literal, so `[]]` is the class
                        // holding `]`; a class with no closing bracket fails below.
                        ']' if class_has_content(&class) => {
                            closed = true;
                            break;
                        }
                        // A middle `-` opens a range; first or last in the class
                        // it is literal. The regex grammar reads each position
                        // the same way, so all three publish unescaped.
                        '-' if prev.is_some() => {
                            match chars.peek() {
                                Some((_, ']')) | None => {
                                    class.push('-');
                                    prev = Some('-');
                                }
                                // A quoted endpoint cannot be ordered against,
                                // so this range escapes validation below.
                                Some((_, '\\')) => {
                                    class.push('-');
                                    prev = None;
                                }
                                Some((_, end)) => {
                                    let end = *end;
                                    match prev {
                                        Some(start) if start > end => {
                                            return Err(format!(
                                                "Descending range '{start}-{end}' at byte {pos} in glob pattern: {glob}"
                                            ));
                                        }
                                        _ => {
                                            class.push('-');
                                            prev = None;
                                        }
                                    }
                                }
                            }
                        }
                        '\\' => match chars.next() {
                            Some((_, quoted)) => {
                                push_class_literal(&mut class, quoted);
                                prev = Some(quoted);
                            }
                            None => {
                                return Err(format!(
                                    "Trailing backslash in character class of glob pattern: {glob}"
                                ));
                            }
                        },
                        _ => {
                            push_class_literal(&mut class, inner);
                            prev = Some(inner);
                        }
                    }
                }
                if !closed {
                    return Err(format!(
                        "Unterminated character class at byte {pos} in glob pattern: {glob}"
                    ));
                }
                out.push('[');
                out.push_str(&class);
                out.push(']');
            }
            _ => push_regex_literal(&mut out, c),
        }
    }
    out.push_str(")$");
    Ok(out)
}

/// Whether a partially built class body already holds content (`^` alone is negation).
fn class_has_content(class: &str) -> bool {
    !(class.is_empty() || class == "^")
}

/// Push a literal character into a regex, escaping every metacharacter.
fn push_regex_literal(out: &mut String, c: char) {
    if matches!(
        c,
        '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
    ) {
        out.push('\\');
    }
    out.push(c);
}

/// Push a literal character into a regex character class, escaping what is special there.
///
/// `-` needs no escape: literal by position at the class edges, a range operator in the
/// middle, and the regex grammar agrees on each. The class parser handles `-` positionally
/// before this is ever reached with one.
fn push_class_literal(class: &mut String, c: char) {
    if matches!(c, ']' | '\\' | '^') {
        class.push('\\');
    }
    class.push(c);
}

/// Query filters for file metadata searches.
#[derive(Clone, Debug, Default)]
pub struct QueryFilters {
    /// Regex pattern to match file paths.
    pub pattern: Option<String>,
    /// Original user text of the pattern filter when given as a glob. The query always
    /// runs on `pattern`; this exists so displays show what the user typed instead of the
    /// translation. Unset when the pattern was given as a regex.
    pub pattern_display: Option<String>,
    /// Minimum file size in bytes.
    pub min_size: Option<i64>,
    /// Maximum file size in bytes.
    pub max_size: Option<i64>,
    /// Files not accessed since this epoch timestamp.
    pub older_than: Option<i64>,
    /// Files accessed since this epoch timestamp.
    pub newer_than: Option<i64>,
    /// Files owned by this uid, resolved from `--owner` at filter-build time.
    pub owner_uid: Option<u32>,
    /// Files owned by this gid, resolved from `--group` at filter-build time.
    pub group_gid: Option<u32>,
    /// Permission-bits predicate, parsed from `--mode` before any query is built.
    pub mode: Option<ModePredicate>,
    /// Files not modified since this epoch timestamp.
    pub mtime_older_than: Option<i64>,
    /// Files modified since this epoch timestamp.
    pub mtime_newer_than: Option<i64>,
}

impl QueryFilters {
    /// Create a new empty filter set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set pattern filter from regex string.
    pub fn with_pattern(mut self, pattern: Option<String>) -> Self {
        self.pattern = pattern;
        self
    }

    /// Set the path pattern filter, interpreting it as a glob unless `regex` holds.
    ///
    /// A glob is translated to an anchored regular expression before storage, so the
    /// query builder never learns a second dialect. An invalid glob fails here — before
    /// any query is built — rather than at the database.
    pub fn with_path_pattern(
        mut self,
        pattern: Option<String>,
        regex: bool,
    ) -> Result<Self, String> {
        match pattern {
            None => Ok(self),
            Some(raw) if regex => {
                self.pattern = Some(raw);
                Ok(self)
            }
            Some(glob) => {
                let translated = glob_to_regex(&glob)?;
                self.pattern = Some(translated);
                self.pattern_display = Some(glob);
                Ok(self)
            }
        }
    }

    /// Set minimum size filter from human-readable string (e.g., "1M").
    pub fn with_min_size(mut self, size: Option<&str>) -> Result<Self, String> {
        self.min_size = size.map(parse_size).transpose()?;
        Ok(self)
    }

    /// Set maximum size filter from human-readable string (e.g., "1G").
    pub fn with_max_size(mut self, size: Option<&str>) -> Result<Self, String> {
        self.max_size = size.map(parse_size).transpose()?;
        Ok(self)
    }

    /// Set older-than filter from days.
    pub fn with_older_than(mut self, days: Option<u64>) -> Self {
        if let Some(d) = days {
            self.older_than = Some(days_ago_epoch(d));
        }
        self
    }

    /// Set newer-than filter from days.
    pub fn with_newer_than(mut self, days: Option<u64>) -> Self {
        if let Some(d) = days {
            self.newer_than = Some(days_ago_epoch(d));
        }
        self
    }

    /// Set owner filter from an already-resolved uid.
    pub fn with_owner_uid(mut self, uid: Option<u32>) -> Self {
        self.owner_uid = uid;
        self
    }

    /// Set group filter from an already-resolved gid.
    pub fn with_group_gid(mut self, gid: Option<u32>) -> Self {
        self.group_gid = gid;
        self
    }

    /// Set permission filter from a `--mode` SPEC string (e.g., "644", "/002").
    ///
    /// Parsing fails here — before any query is built — rather than at the database.
    pub fn with_mode(mut self, spec: Option<&str>) -> Result<Self, String> {
        self.mode = spec.map(parse_mode_spec).transpose()?;
        Ok(self)
    }

    /// Set mtime-older-than filter from days, the modification-time analogue of
    /// `with_older_than`.
    pub fn with_mtime_older_than(mut self, days: Option<u64>) -> Self {
        if let Some(d) = days {
            self.mtime_older_than = Some(days_ago_epoch(d));
        }
        self
    }

    /// Set mtime-newer-than filter from days, the modification-time analogue of
    /// `with_newer_than`.
    pub fn with_mtime_newer_than(mut self, days: Option<u64>) -> Self {
        if let Some(d) = days {
            self.mtime_newer_than = Some(days_ago_epoch(d));
        }
        self
    }

    /// Returns true if any filter is active.
    pub fn is_active(&self) -> bool {
        self.pattern.is_some()
            || self.min_size.is_some()
            || self.max_size.is_some()
            || self.older_than.is_some()
            || self.newer_than.is_some()
            || self.owner_uid.is_some()
            || self.group_gid.is_some()
            || self.mode.is_some()
            || self.mtime_older_than.is_some()
            || self.mtime_newer_than.is_some()
    }

    /// Returns individual WHERE clause conditions.
    pub fn to_conditions(&self) -> Vec<String> {
        let mut conditions = Vec::new();

        if let Some(ref pattern) = self.pattern {
            let escaped = pattern.replace('\'', "''");
            conditions.push(format!("regexp_matches(path, '{}')", escaped));
        }

        if let Some(min_size) = self.min_size {
            conditions.push(format!("size >= {}", min_size));
        }

        if let Some(max_size) = self.max_size {
            conditions.push(format!("size <= {}", max_size));
        }

        if let Some(threshold) = self.older_than {
            conditions.push(format!("atime < {}", threshold));
        }

        if let Some(threshold) = self.newer_than {
            conditions.push(format!("atime >= {}", threshold));
        }

        if let Some(uid) = self.owner_uid {
            conditions.push(format!("uid = {}", uid));
        }

        if let Some(gid) = self.group_gid {
            conditions.push(format!("gid = {}", gid));
        }

        if let Some(pred) = self.mode {
            conditions.push(match pred {
                ModePredicate::Exact(bits) => format!("mode = {}", bits),
                ModePredicate::Any(bits) => format!("(mode & {}) != 0", bits),
                ModePredicate::All(bits) => format!("(mode & {}) = {}", bits, bits),
            });
        }

        if let Some(threshold) = self.mtime_older_than {
            conditions.push(format!("mtime < {}", threshold));
        }

        if let Some(threshold) = self.mtime_newer_than {
            conditions.push(format!("mtime >= {}", threshold));
        }

        conditions
    }

    /// Returns a WHERE clause string (without "WHERE" prefix).
    /// Returns empty string if no filters are active.
    pub fn to_where_clause(&self) -> String {
        let conditions = self.to_conditions();
        if conditions.is_empty() {
            String::new()
        } else {
            conditions.join(" AND ")
        }
    }

    /// Returns a full WHERE clause string (with "WHERE" prefix).
    /// Returns empty string if no filters are active.
    pub fn to_full_where_clause(&self) -> String {
        let clause = self.to_where_clause();
        if clause.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clause)
        }
    }

    /// Clear all filters.
    pub fn clear(&mut self) {
        self.pattern = None;
        self.pattern_display = None;
        self.min_size = None;
        self.max_size = None;
        self.older_than = None;
        self.newer_than = None;
        self.owner_uid = None;
        self.group_gid = None;
        self.mode = None;
        self.mtime_older_than = None;
        self.mtime_newer_than = None;
    }

    /// Format active filters for display (e.g., "[older:30d] [min:1M]").
    pub fn format_display(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref display) = self.pattern_display.as_ref().or(self.pattern.as_ref()) {
            parts.push(format!("[/{}]", display));
        }

        if let Some(min_size) = self.min_size {
            parts.push(format!("[min:{}]", format_bytes(min_size as u64)));
        }

        if let Some(max_size) = self.max_size {
            parts.push(format!("[max:{}]", format_bytes(max_size as u64)));
        }

        if let Some(threshold) = self.older_than {
            parts.push(format!("[older:{}d]", days_since_epoch(threshold)));
        }

        if let Some(threshold) = self.newer_than {
            parts.push(format!("[newer:{}d]", days_since_epoch(threshold)));
        }

        if let Some(uid) = self.owner_uid {
            parts.push(format!("[owner:{}]", uid));
        }

        if let Some(gid) = self.group_gid {
            parts.push(format!("[group:{}]", gid));
        }

        if let Some(pred) = self.mode {
            parts.push(format!("[mode:{}]", pred));
        }

        if let Some(threshold) = self.mtime_older_than {
            parts.push(format!("[mtime-older:{}d]", days_since_epoch(threshold)));
        }

        if let Some(threshold) = self.mtime_newer_than {
            parts.push(format!("[mtime-newer:{}d]", days_since_epoch(threshold)));
        }

        parts.join(" ")
    }
}

/// A parsed `--mode` permission filter: an operator plus permission bits.
///
/// Exact answers "the mode is this"; Any answers "any of these bits is set" (the
/// exposure question — world-writable is Any(`0o002`)); All answers "every one of
/// these bits is set" (setuid is All(`0o4000`)). Bits are the `0o7777` permission
/// field, matching the stored `mode` column, so SQL compares them bare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModePredicate {
    Exact(u32),
    Any(u32),
    All(u32),
}

impl fmt::Display for ModePredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModePredicate::Exact(bits) => write!(f, "{:o}", bits),
            ModePredicate::Any(bits) => write!(f, "/{:o}", bits),
            ModePredicate::All(bits) => write!(f, "&{:o}", bits),
        }
    }
}

/// Parse a `--mode` SPEC into an operator plus bits.
///
/// Grammar: bare octal (`644`, `0644`) is exact; a `/` prefix (`/002`) matches
/// when any listed bit is set; an `&` prefix (`&4000`) matches when all listed
/// bits are set. A leading `-` is not accepted — clap would eat it as a flag —
/// and `&` is its spelling instead. An optional `0o` may follow the prefix.
/// Anything else — empty input, a bare prefix, non-octal digits, more than
/// `0o7777` — fails before any query is built.
pub fn parse_mode_spec(spec: &str) -> Result<ModePredicate, String> {
    let (op, digits) = match spec.strip_prefix('/') {
        Some(rest) => ('/', rest),
        None => match spec.strip_prefix('&') {
            Some(rest) => ('&', rest),
            None => ('=', spec),
        },
    };
    let digits = digits.strip_prefix("0o").unwrap_or(digits);
    if digits.is_empty() || !digits.bytes().all(|b| matches!(b, b'0'..=b'7')) {
        return Err(format!(
            "invalid mode SPEC: '{spec}' (want octal like 644, /002, or &4000)"
        ));
    }
    let bits = u32::from_str_radix(digits, 8).map_err(|_| {
        format!("invalid mode SPEC: '{spec}' (want octal like 644, /002, or &4000)")
    })?;
    if bits > 0o7777 {
        return Err(format!(
            "invalid mode SPEC: '{spec}' (permission bits stop at 07777)"
        ));
    }
    Ok(match op {
        '/' => ModePredicate::Any(bits),
        '&' => ModePredicate::All(bits),
        _ => ModePredicate::Exact(bits),
    })
}

/// Resolve a user name to its uid, POSIX `find -user` style.
///
/// A name present in the user database wins — even a numeric one. Otherwise an
/// all-digit string parses as a literal uid, so a host missing the passwd entry
/// can still select by number. Anything else fails.
///
/// This calls the non-reentrant `getpwnam`, keeps no pointer past the call, and
/// still must not run on a thread pool: the one call site resolves filters on
/// the main thread before any query runs.
pub fn resolve_user(name: &str) -> Result<u32, String> {
    let cname = CString::new(name).map_err(|_| format!("unknown user: '{name}'"))?;
    let uid = unsafe {
        let entry = libc::getpwnam(cname.as_ptr());
        if entry.is_null() {
            None
        } else {
            Some((*entry).pw_uid)
        }
    };
    if let Some(uid) = uid {
        return Ok(uid);
    }
    decimal_id(name).ok_or_else(|| format!("unknown user: '{name}'"))
}

/// Resolve a group name to its gid: the group mirror of `resolve_user`, over
/// `getgrnam`, with the same lookup-first-then-digits policy and the same
/// single-threaded call-site constraint.
pub fn resolve_group(name: &str) -> Result<u32, String> {
    let cname = CString::new(name).map_err(|_| format!("unknown group: '{name}'"))?;
    let gid = unsafe {
        let entry = libc::getgrnam(cname.as_ptr());
        if entry.is_null() {
            None
        } else {
            Some((*entry).gr_gid)
        }
    };
    if let Some(gid) = gid {
        return Ok(gid);
    }
    decimal_id(name).ok_or_else(|| format!("unknown group: '{name}'"))
}

/// A digit string as a literal id. Empty strings, signs, and mixed tokens are
/// not ids; a value past `u32` is not one either.
fn decimal_id(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok()
}

/// Unix epoch seconds `days` whole days before now: the shared conversion behind
/// every day-count CLI filter, atime and mtime alike.
fn days_ago_epoch(days: u64) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    now - (days as i64 * 86400)
}

/// Whole days between a stored epoch threshold and now, for filter displays.
fn days_since_epoch(threshold: i64) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    (now - threshold) / 86400
}

/// Build the deterministic `ORDER BY path LIMIT n` tail for a capped query.
///
/// A bare `LIMIT` returns an arbitrary, unstable subset, so ordering and limiting are
/// inseparable: whenever a limit is present the query orders by `path` — the unique key of
/// the index — so a `--dry-run` preview and the subsequent real deletion select identical
/// rows. With no limit the tail is empty: every match is acted on regardless of order, and
/// ordering the whole match set would be wasted work.
pub fn deterministic_limit_clause(limit: Option<usize>) -> String {
    match limit {
        Some(n) => format!("ORDER BY path LIMIT {n}"),
        None => String::new(),
    }
}

/// Returns the Arrow schema for file metadata records.
///
/// Identity columns first (`path`, `size`, `uid`, `gid`, `mode`), then the three
/// clocks (`atime`, `mtime`, `ctime`). Physical order binds no reader — every query
/// projects by name, and the version gate refuses cross-version indexes — so the
/// layout follows semantics rather than history.
pub fn get_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("path", DataType::Utf8, false),
        Field::new("size", DataType::Int64, false),
        Field::new("uid", DataType::Int64, false),
        Field::new("gid", DataType::Int64, false),
        Field::new("mode", DataType::Int64, false),
        Field::new("atime", DataType::Int64, false),
        Field::new("mtime", DataType::Int64, false),
        Field::new("ctime", DataType::Int64, false),
    ]))
}

/// Format a count with human-readable suffixes (K, M, B).
pub fn format_count(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

/// Format crawl speed as human-readable files/s.
pub fn format_speed(files_per_sec: f64) -> String {
    if files_per_sec >= 1_000_000.0 {
        format!("{:.1}M files/s", files_per_sec / 1_000_000.0)
    } else if files_per_sec >= 1_000.0 {
        format!("{:.1}k files/s", files_per_sec / 1_000.0)
    } else {
        format!("{:.0} files/s", files_per_sec)
    }
}

/// Format bytes with binary suffixes (KiB, MiB, GiB, TiB).
pub fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        format!("{:.2} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Render one driver's per-partition progress line from its observed counters.
///
/// The line state derives from the counters, never from a thread claim: zero yielded entries
/// means the driver holds the partition but the walk has produced nothing yet (`waiting`);
/// entries without completed files mean metadata traversal with nothing countable
/// (`scanning`, with directories visited and elapsed); completed files mean the lively shape.
/// Elapsed arrives as whole seconds so the line stays stable between refresh ticks, and the
/// speed fragment is the caller's verbatim (empty when the rolling window has no sample yet).
pub fn format_partition_progress(
    partition: &str,
    files: u64,
    bytes: u64,
    dirs_visited: u64,
    elapsed_secs: u64,
    speed_fragment: &str,
) -> String {
    if files == 0 && dirs_visited == 0 {
        format!("{}: waiting... {}s", partition, elapsed_secs)
    } else if files == 0 {
        format!(
            "{}: scanning {} dirs, {}s, no files yet",
            partition,
            format_count(dirs_visited),
            elapsed_secs
        )
    } else {
        format!(
            "{}: {} files, {}{}",
            partition,
            format_count(files),
            format_bytes(bytes),
            speed_fragment
        )
    }
}

/// Severity of one stderr log record emitted by a non-TTY run.
///
/// The tag a script greps for. Three levels cover the crawl's vocabulary: routine progress,
/// tolerated anomalies, and run-failing faults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// The tag text rendered into the record line.
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// Render a `SystemTime` as a UTC `YYYY-MM-DDTHH:MM:SSZ` stamp.
///
/// Second precision is the cron-correlation granularity this log promises; ordering within
/// a second is still line order. The conversion is `std` alone: `chrono` arrives only
/// transitively through `arrow`, and one stamp line does not justify promoting it to a
/// direct dependency. A pre-epoch time saturates at the epoch rather than failing the run.
pub fn format_log_timestamp(now: SystemTime) -> String {
    let secs = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let time = secs % 86_400;
    // Days since 1970-01-01 to civil date. The input cannot be negative (pre-epoch
    // saturates above), so Euclidean division agrees with truncation throughout.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        m,
        d,
        time / 3_600,
        (time % 3_600) / 60,
        time % 60
    )
}

/// Render one machine-readable stderr log record: `<timestamp> <LEVEL> <message>`.
///
/// The message tail is the caller's verbatim apart from line-break folding: a record is one
/// line by construction, so embedded breaks (legal in Unix paths) become spaces rather than
/// forging extra lines a script would misread as separate events.
pub fn format_log_record(level: LogLevel, message: &str) -> String {
    let single_line: Vec<&str> = message.split(['\r', '\n']).collect();
    format!(
        "{} {} {}",
        format_log_timestamp(SystemTime::now()),
        level.as_str(),
        single_line.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    #[test]
    fn test_format_count_units() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(1), "1");
        assert_eq!(format_count(999), "999");
    }

    #[test]
    fn test_format_count_thousands() {
        assert_eq!(format_count(1_000), "1.0K");
        assert_eq!(format_count(1_500), "1.5K");
        assert_eq!(format_count(999_999), "1000.0K");
    }

    #[test]
    fn test_format_count_millions() {
        assert_eq!(format_count(1_000_000), "1.0M");
        assert_eq!(format_count(2_500_000), "2.5M");
        assert_eq!(format_count(999_999_999), "1000.0M");
    }

    #[test]
    fn test_format_count_billions() {
        assert_eq!(format_count(1_000_000_000), "1.0B");
        assert_eq!(format_count(5_500_000_000), "5.5B");
    }

    #[test]
    fn test_format_speed_low() {
        assert_eq!(format_speed(0.0), "0 files/s");
        assert_eq!(format_speed(1.0), "1 files/s");
        assert_eq!(format_speed(500.0), "500 files/s");
        assert_eq!(format_speed(999.0), "999 files/s");
    }

    #[test]
    fn test_format_speed_thousands() {
        assert_eq!(format_speed(1_000.0), "1.0k files/s");
        assert_eq!(format_speed(42_500.0), "42.5k files/s");
        assert_eq!(format_speed(127_000.0), "127.0k files/s");
        assert_eq!(format_speed(999_999.0), "1000.0k files/s");
    }

    #[test]
    fn test_format_speed_millions() {
        assert_eq!(format_speed(1_000_000.0), "1.0M files/s");
        assert_eq!(format_speed(2_500_000.0), "2.5M files/s");
    }

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1), "1 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_kib() {
        assert_eq!(format_bytes(1024), "1.00 KiB");
        assert_eq!(format_bytes(1536), "1.50 KiB");
        assert_eq!(format_bytes(1024 * 1023), "1023.00 KiB");
    }

    #[test]
    fn test_format_bytes_mib() {
        assert_eq!(format_bytes(1024 * 1024), "1.00 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 2 + 1024 * 512), "2.50 MiB");
    }

    // format_partition_progress() tests
    #[test]
    fn test_partition_progress_waiting() {
        // Zero yielded entries: the driver holds the partition but the walk has produced
        // nothing, so the line waits rather than borrowing the active word.
        assert_eq!(
            format_partition_progress("alice", 0, 0, 0, 0, ""),
            "alice: waiting... 0s"
        );
        assert_eq!(
            format_partition_progress("alice", 0, 0, 0, 47, ""),
            "alice: waiting... 47s"
        );
    }

    #[test]
    fn test_partition_progress_quiet_scanning() {
        // Entries without completed files: metadata traversal with nothing countable yet.
        assert_eq!(
            format_partition_progress("alice", 0, 0, 3, 12, ""),
            "alice: scanning 3 dirs, 12s, no files yet"
        );
        // Directory counts scale the way file counts do.
        assert_eq!(
            format_partition_progress("alice", 0, 0, 1500, 61, ""),
            "alice: scanning 1.5K dirs, 61s, no files yet"
        );
    }

    #[test]
    fn test_partition_progress_lively() {
        // Completed files: counts and bytes, with the caller's speed fragment verbatim.
        assert_eq!(
            format_partition_progress("alice", 2, 2048, 5, 3, ""),
            "alice: 2 files, 2.00 KiB"
        );
        assert_eq!(
            format_partition_progress(
                "alice",
                1500,
                4096,
                9,
                3,
                " | 1.5k files/s (peak: 2.0k files/s)"
            ),
            "alice: 1.5K files, 4.00 KiB | 1.5k files/s (peak: 2.0k files/s)"
        );
    }

    #[test]
    fn test_partition_progress_carries_no_thread_claim() {
        // The regression this cycle exists for: no line state may name a driver or worker.
        let states = [
            format_partition_progress("alice", 0, 0, 0, 30, ""),
            format_partition_progress("alice", 0, 0, 500, 30, ""),
            format_partition_progress("alice", 10, 1024, 5, 30, ""),
        ];
        for msg in &states {
            assert!(!msg.contains("[T"), "thread claim leaked: {msg}");
        }
    }

    #[test]
    fn test_format_bytes_gib() {
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GiB");
        assert_eq!(
            format_bytes(1024 * 1024 * 1024 * 3 + 1024 * 1024 * 512),
            "3.50 GiB"
        );
    }

    #[test]
    fn test_format_bytes_tib() {
        assert_eq!(format_bytes(1024_u64 * 1024 * 1024 * 1024), "1.00 TiB");
        assert_eq!(
            format_bytes(1024_u64 * 1024 * 1024 * 1024 * 2 + 1024_u64 * 1024 * 1024 * 512),
            "2.50 TiB"
        );
    }

    #[test]
    fn test_log_level_tags() {
        assert_eq!(LogLevel::Info.as_str(), "INFO");
        assert_eq!(LogLevel::Warn.as_str(), "WARN");
        assert_eq!(LogLevel::Error.as_str(), "ERROR");
    }

    #[test]
    fn test_format_log_timestamp_epoch() {
        assert_eq!(format_log_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_format_log_timestamp_known_dates() {
        let at =
            |secs: u64| format_log_timestamp(UNIX_EPOCH + std::time::Duration::from_secs(secs));
        assert_eq!(at(946_684_799), "1999-12-31T23:59:59Z");
        assert_eq!(at(951_868_799), "2000-02-29T23:59:59Z");
        assert_eq!(at(1_709_210_096), "2024-02-29T12:34:56Z");
        assert_eq!(at(1_789_009_201), "2026-09-10T03:00:01Z");
    }

    #[test]
    fn test_format_log_record_shape() {
        let record = format_log_record(LogLevel::Info, "Indexing /data");
        assert!(
            record.ends_with(" INFO Indexing /data"),
            "tail stays verbatim: {record}"
        );
        let stamp = &record[..20];
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[7..8], "-");
        assert_eq!(&stamp[10..11], "T");
        assert_eq!(&stamp[13..14], ":");
        assert_eq!(&stamp[16..17], ":");
        assert_eq!(&stamp[19..20], "Z");
        assert!(
            !record.contains('\n') && !record.contains('\r'),
            "one record is one line: {record:?}"
        );
    }

    #[test]
    fn test_format_log_record_folds_embedded_breaks() {
        let record = format_log_record(LogLevel::Error, "error: a\nb\rc: denied");
        assert!(
            record.ends_with(" ERROR error: a b c: denied"),
            "breaks fold to spaces: {record:?}"
        );
    }

    #[test]
    fn test_schema_fields() {
        let schema = get_schema();
        let names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(
            names,
            vec![
                "path", "size", "uid", "gid", "mode", "atime", "mtime", "ctime"
            ]
        );
        for field in schema.fields() {
            if field.name() == "path" {
                assert_eq!(field.data_type(), &DataType::Utf8);
            } else {
                assert_eq!(field.data_type(), &DataType::Int64);
            }
            assert!(!field.is_nullable());
        }
    }

    // SizeMode::calculate() tests
    #[test]
    fn test_size_mode_disk_usage() {
        let mode = SizeMode::DiskUsage;
        // disk_usage = 8192 (16 blocks * 512), file_len = 5000
        assert_eq!(mode.calculate(8192, 5000), 8192);
        assert_eq!(mode.calculate(0, 1000), 0);
        assert_eq!(mode.calculate(512, 100), 512);
    }

    #[test]
    fn test_size_mode_apparent_size() {
        let mode = SizeMode::ApparentSize;
        // Should always return file_len regardless of disk_usage
        assert_eq!(mode.calculate(8192, 5000), 5000);
        assert_eq!(mode.calculate(0, 1000), 1000);
        assert_eq!(mode.calculate(512, 100), 100);
    }

    #[test]
    fn test_size_mode_block_rounded() {
        // 4K block size
        let mode = SizeMode::BlockRounded(4096);
        // 5000 bytes rounds up to 8192 (2 blocks)
        assert_eq!(mode.calculate(8192, 5000), 8192);
        // 4096 exactly stays at 4096
        assert_eq!(mode.calculate(4096, 4096), 4096);
        // 1 byte rounds up to 4096
        assert_eq!(mode.calculate(512, 1), 4096);
        // 0 bytes stays 0
        assert_eq!(mode.calculate(0, 0), 0);
    }

    #[test]
    fn test_size_mode_block_rounded_various_sizes() {
        let mode = SizeMode::BlockRounded(1024); // 1K blocks
        assert_eq!(mode.calculate(0, 1), 1024);
        assert_eq!(mode.calculate(0, 1024), 1024);
        assert_eq!(mode.calculate(0, 1025), 2048);
        assert_eq!(mode.calculate(0, 2048), 2048);

        // 128K blocks (common HPC block size)
        let mode = SizeMode::BlockRounded(131072);
        assert_eq!(mode.calculate(0, 1), 131072);
        assert_eq!(mode.calculate(0, 131072), 131072);
        assert_eq!(mode.calculate(0, 131073), 262144);
    }

    // round_to_block() tests
    #[test]
    fn test_round_to_block_basic() {
        assert_eq!(round_to_block(0, 4096), 0);
        assert_eq!(round_to_block(1, 4096), 4096);
        assert_eq!(round_to_block(4096, 4096), 4096);
        assert_eq!(round_to_block(4097, 4096), 8192);
    }

    #[test]
    fn test_round_to_block_zero_block_size() {
        // Zero block size should return size unchanged
        assert_eq!(round_to_block(100, 0), 100);
        assert_eq!(round_to_block(0, 0), 0);
    }

    #[test]
    fn test_round_to_block_large_sizes() {
        // 1 MiB block size
        let mb = 1024 * 1024;
        assert_eq!(round_to_block(1, mb), mb);
        assert_eq!(round_to_block(mb, mb), mb);
        assert_eq!(round_to_block(mb + 1, mb), 2 * mb);
    }

    // parse_size() tests
    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("100").unwrap(), 100);
        assert_eq!(parse_size("100B").unwrap(), 100);
        assert_eq!(parse_size("0").unwrap(), 0);
    }

    #[test]
    fn test_parse_size_kilobytes() {
        assert_eq!(parse_size("1K").unwrap(), 1024);
        assert_eq!(parse_size("1KiB").unwrap(), 1024);
        assert_eq!(parse_size("2.5K").unwrap(), 2560);
    }

    #[test]
    fn test_parse_size_megabytes() {
        assert_eq!(parse_size("1M").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("1MiB").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("10M").unwrap(), 10 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_gigabytes() {
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("1GiB").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_terabytes() {
        assert_eq!(parse_size("1T").unwrap(), 1024_i64 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("1TiB").unwrap(), 1024_i64 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_case_insensitive() {
        assert_eq!(parse_size("1k").unwrap(), 1024);
        assert_eq!(parse_size("1m").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("1g").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_invalid() {
        assert!(parse_size("abc").is_err());
        assert!(parse_size("K").is_err());
    }

    // SortMode tests
    #[test]
    fn test_sort_mode_default() {
        let mode = SortMode::default();
        assert_eq!(mode, SortMode::Name);
    }

    #[test]
    fn test_sort_mode_cycle() {
        assert_eq!(SortMode::Name.next(), SortMode::SizeDesc);
        assert_eq!(SortMode::SizeDesc.next(), SortMode::SizeAsc);
        assert_eq!(SortMode::SizeAsc.next(), SortMode::CountDesc);
        assert_eq!(SortMode::CountDesc.next(), SortMode::CountAsc);
        assert_eq!(SortMode::CountAsc.next(), SortMode::AgeDesc);
        assert_eq!(SortMode::AgeDesc.next(), SortMode::AgeAsc);
        assert_eq!(SortMode::AgeAsc.next(), SortMode::Name);
    }

    #[test]
    fn test_sort_mode_from_str() {
        assert_eq!("name".parse::<SortMode>().unwrap(), SortMode::Name);
        assert_eq!("size-desc".parse::<SortMode>().unwrap(), SortMode::SizeDesc);
        assert_eq!("size".parse::<SortMode>().unwrap(), SortMode::SizeDesc);
        assert_eq!("size-asc".parse::<SortMode>().unwrap(), SortMode::SizeAsc);
        assert_eq!(
            "count-desc".parse::<SortMode>().unwrap(),
            SortMode::CountDesc
        );
        assert_eq!("count".parse::<SortMode>().unwrap(), SortMode::CountDesc);
        assert_eq!("count-asc".parse::<SortMode>().unwrap(), SortMode::CountAsc);
        assert_eq!("age-desc".parse::<SortMode>().unwrap(), SortMode::AgeDesc);
        assert_eq!("age".parse::<SortMode>().unwrap(), SortMode::AgeDesc);
        assert_eq!("oldest".parse::<SortMode>().unwrap(), SortMode::AgeDesc);
        assert_eq!("age-asc".parse::<SortMode>().unwrap(), SortMode::AgeAsc);
        assert_eq!("newest".parse::<SortMode>().unwrap(), SortMode::AgeAsc);
    }

    #[test]
    fn test_sort_mode_from_str_invalid() {
        assert!("invalid".parse::<SortMode>().is_err());
    }

    #[test]
    fn test_sort_mode_display() {
        assert_eq!(SortMode::Name.to_string(), "name");
        assert_eq!(SortMode::SizeDesc.to_string(), "size-desc");
        assert_eq!(SortMode::SizeAsc.to_string(), "size-asc");
        assert_eq!(SortMode::CountDesc.to_string(), "count-desc");
        assert_eq!(SortMode::CountAsc.to_string(), "count-asc");
        assert_eq!(SortMode::AgeDesc.to_string(), "age-desc");
        assert_eq!(SortMode::AgeAsc.to_string(), "age-asc");
    }

    #[test]
    fn test_sort_mode_order_by() {
        assert_eq!(
            SortMode::Name.to_order_by(true),
            "bool_or(is_dir) DESC, component"
        );
        assert_eq!(SortMode::Name.to_order_by(false), "component");
        assert_eq!(SortMode::SizeDesc.to_order_by(true), "total_size DESC");
        assert_eq!(SortMode::SizeAsc.to_order_by(false), "total_size ASC");
        assert_eq!(SortMode::AgeDesc.to_order_by(false), "latest_atime ASC");
        assert_eq!(SortMode::AgeAsc.to_order_by(false), "latest_atime DESC");
    }

    // QueryFilters tests
    #[test]
    fn test_query_filters_empty() {
        let filters = QueryFilters::new();
        assert!(!filters.is_active());
        assert_eq!(filters.to_where_clause(), "");
        assert_eq!(filters.to_full_where_clause(), "");
    }

    #[test]
    fn test_query_filters_pattern() {
        let filters = QueryFilters::new().with_pattern(Some("\\.py$".to_string()));
        assert!(filters.is_active());
        assert!(filters.to_where_clause().contains("regexp_matches"));
        assert!(filters.to_where_clause().contains(".py$"));
    }

    #[test]
    fn test_glob_to_regex_star_crosses_directories() {
        assert_eq!(glob_to_regex("*.py"), Ok("^(?:.*\\.py)$".to_string()));
    }

    #[test]
    fn test_glob_to_regex_literals_are_escaped() {
        assert_eq!(
            glob_to_regex("data(2024).log"),
            Ok("^(?:data\\(2024\\)\\.log)$".to_string())
        );
    }

    #[test]
    fn test_glob_to_regex_question_and_class() {
        assert_eq!(
            glob_to_regex("file?.[ch]"),
            Ok("^(?:file.\\.[ch])$".to_string())
        );
        assert_eq!(glob_to_regex("[!a]*"), Ok("^(?:[^a].*)$".to_string()));
    }

    #[test]
    fn test_glob_to_regex_backslash_quotes() {
        assert_eq!(glob_to_regex("a\\*b"), Ok("^(?:a\\*b)$".to_string()));
    }

    #[test]
    fn test_glob_to_regex_rejections() {
        assert!(glob_to_regex("").is_err());
        assert!(glob_to_regex("[abc").is_err());
        assert!(glob_to_regex("[]").is_err());
        assert!(glob_to_regex("abc\\").is_err());
        assert!(glob_to_regex("a[b\\").is_err());
    }

    #[test]
    fn test_glob_to_regex_class_ranges() {
        assert_eq!(
            glob_to_regex("*.[a-z]og"),
            Ok("^(?:.*\\.[a-z]og)$".to_string())
        );
        assert_eq!(glob_to_regex("[a-m]*"), Ok("^(?:[a-m].*)$".to_string()));
        assert_eq!(glob_to_regex("[!0-9]*"), Ok("^(?:[^0-9].*)$".to_string()));
    }

    #[test]
    fn test_glob_to_regex_dash_literal_at_class_edges() {
        assert_eq!(glob_to_regex("[-a]"), Ok("^(?:[-a])$".to_string()));
        assert_eq!(glob_to_regex("[a-]"), Ok("^(?:[a-])$".to_string()));
        assert_eq!(glob_to_regex("[!-a]"), Ok("^(?:[^-a])$".to_string()));
    }

    #[test]
    fn test_glob_to_regex_descending_range_rejected() {
        let err = glob_to_regex("*.[z-a]").unwrap_err();
        assert!(err.contains("Descending range"), "unexpected: {err}");
    }

    #[test]
    fn test_with_path_pattern_glob_and_regex() {
        let globbed = QueryFilters::new()
            .with_path_pattern(Some("*.py".to_string()), false)
            .unwrap();
        assert_eq!(globbed.pattern, Some("^(?:.*\\.py)$".to_string()));
        assert_eq!(globbed.pattern_display, Some("*.py".to_string()));
        assert!(globbed.to_where_clause().contains("regexp_matches"));
        assert!(globbed.format_display().contains("[/*.py]"));

        let raw = QueryFilters::new()
            .with_path_pattern(Some("\\.py$".to_string()), true)
            .unwrap();
        assert_eq!(raw.pattern, Some("\\.py$".to_string()));
        assert_eq!(raw.pattern_display, None);

        assert!(
            QueryFilters::new()
                .with_path_pattern(Some("[abc".to_string()), false)
                .is_err()
        );
    }

    #[test]
    fn test_query_filters_size() {
        let filters = QueryFilters::new()
            .with_min_size(Some("1M"))
            .unwrap()
            .with_max_size(Some("1G"))
            .unwrap();
        assert!(filters.is_active());
        let clause = filters.to_where_clause();
        assert!(clause.contains("size >= 1048576"));
        assert!(clause.contains("size <= 1073741824"));
    }

    #[test]
    fn test_query_filters_combined() {
        let filters = QueryFilters::new()
            .with_pattern(Some("test".to_string()))
            .with_min_size(Some("1K"))
            .unwrap();
        let clause = filters.to_where_clause();
        assert!(clause.contains("AND"));
        assert!(clause.contains("regexp_matches"));
        assert!(clause.contains("size >= 1024"));
    }

    #[test]
    fn test_query_filters_clear() {
        let mut filters = QueryFilters::new()
            .with_pattern(Some("test".to_string()))
            .with_min_size(Some("1K"))
            .unwrap();
        assert!(filters.is_active());
        filters.clear();
        assert!(!filters.is_active());
    }

    #[test]
    fn test_query_filters_full_where_clause() {
        let filters = QueryFilters::new().with_min_size(Some("1M")).unwrap();
        let clause = filters.to_full_where_clause();
        assert!(clause.starts_with("WHERE "));
    }

    #[test]
    fn test_query_filters_owner_group_fragments() {
        let filters = QueryFilters::new()
            .with_owner_uid(Some(1000))
            .with_group_gid(Some(100));
        assert!(filters.is_active());
        let clause = filters.to_where_clause();
        assert!(clause.contains("uid = 1000"));
        assert!(clause.contains("gid = 100"));
        assert!(clause.contains("AND"));
    }

    #[test]
    fn test_query_filters_mode_fragments() {
        let exact = QueryFilters::new().with_mode(Some("644")).unwrap();
        assert!(exact.to_where_clause().contains("mode = 420"));

        let any = QueryFilters::new().with_mode(Some("/002")).unwrap();
        assert!(any.to_where_clause().contains("(mode & 2) != 0"));

        let all = QueryFilters::new().with_mode(Some("&4000")).unwrap();
        assert!(all.to_where_clause().contains("(mode & 2048) = 2048"));

        assert!(QueryFilters::new().with_mode(Some("888")).is_err());
        assert!(QueryFilters::new().with_mode(Some("-002")).is_err());
    }

    #[test]
    fn test_parse_mode_spec_matrix() {
        assert_eq!(parse_mode_spec("644"), Ok(ModePredicate::Exact(0o644)));
        assert_eq!(parse_mode_spec("0644"), Ok(ModePredicate::Exact(0o644)));
        assert_eq!(parse_mode_spec("0o755"), Ok(ModePredicate::Exact(0o755)));
        assert_eq!(parse_mode_spec("/002"), Ok(ModePredicate::Any(0o002)));
        assert_eq!(parse_mode_spec("&4000"), Ok(ModePredicate::All(0o4000)));
        assert_eq!(parse_mode_spec("0"), Ok(ModePredicate::Exact(0)));
        assert_eq!(parse_mode_spec("7777"), Ok(ModePredicate::Exact(0o7777)));

        for bad in [
            "", "/", "&", "8", "888", "10000", "-002", "u=rwx", "64 4", "0x10",
        ] {
            assert!(parse_mode_spec(bad).is_err(), "SPEC '{bad}' must fail");
        }
    }

    #[test]
    fn test_query_filters_mtime_fragments() {
        let filters = QueryFilters::new()
            .with_mtime_older_than(Some(30))
            .with_mtime_newer_than(Some(7));
        assert!(filters.is_active());
        let clause = filters.to_where_clause();
        assert!(clause.contains("mtime < "));
        assert!(clause.contains("mtime >= "));
        assert!(clause.contains("AND"));

        let idle = QueryFilters::new()
            .with_mtime_older_than(None)
            .with_mtime_newer_than(None);
        assert!(!idle.is_active());
    }

    #[test]
    fn test_query_filters_new_fields_clear_and_display() {
        let mut filters = QueryFilters::new()
            .with_owner_uid(Some(1000))
            .with_group_gid(Some(100))
            .with_mode(Some("/002"))
            .unwrap()
            .with_mtime_older_than(Some(30));
        let display = filters.format_display();
        assert!(display.contains("[owner:1000]"));
        assert!(display.contains("[group:100]"));
        assert!(display.contains("[mode:/2]"));
        assert!(display.contains("[mtime-older:30d]"));
        filters.clear();
        assert!(!filters.is_active());
        assert_eq!(filters.to_where_clause(), "");
    }

    #[test]
    fn test_resolve_user_lookup_first_then_digits() {
        let euid = unsafe { libc::geteuid() };
        let name = unsafe {
            let entry = libc::getpwuid(euid);
            assert!(!entry.is_null());
            std::ffi::CStr::from_ptr((*entry).pw_name)
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(resolve_user(&name), Ok(euid));

        assert!(resolve_user("xdu-no-such-user").is_err());
        assert!(resolve_user("").is_err());
        assert!(resolve_user("-1").is_err());
        assert!(resolve_user("1000x").is_err());
        assert!(resolve_user("99999999999").is_err());
        // A pure digit string with no such name is the uid itself.
        assert_eq!(resolve_user("4294967294"), Ok(4294967294));
    }

    #[test]
    fn test_resolve_group_lookup_first_then_digits() {
        let egid = unsafe { libc::getegid() };
        let name = unsafe {
            let entry = libc::getgrgid(egid);
            assert!(!entry.is_null());
            std::ffi::CStr::from_ptr((*entry).gr_name)
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(resolve_group(&name), Ok(egid));

        assert!(resolve_group("xdu-no-such-group").is_err());
        assert!(resolve_group("").is_err());
        assert!(resolve_group("100x").is_err());
        assert_eq!(resolve_group("4294967294"), Ok(4294967294));
    }

    // deterministic_limit_clause() tests
    #[test]
    fn test_deterministic_limit_clause_none() {
        assert_eq!(deterministic_limit_clause(None), "");
    }

    #[test]
    fn test_deterministic_limit_clause_some() {
        assert_eq!(deterministic_limit_clause(Some(1)), "ORDER BY path LIMIT 1");
        assert_eq!(deterministic_limit_clause(Some(5)), "ORDER BY path LIMIT 5");
    }

    // index layout: the glob every reader shares

    #[test]
    fn test_index_glob_all_partitions() {
        assert_eq!(
            index_glob(Path::new("/index/scratch"), None),
            "/index/scratch/*/*.parquet"
        );
    }

    #[test]
    fn test_index_glob_single_partition() {
        assert_eq!(
            index_glob(Path::new("/index/scratch"), Some("alice")),
            "/index/scratch/alice/*.parquet"
        );
        // The reserved loose-file partition is addressed like any other.
        assert_eq!(
            index_glob(Path::new("/index/scratch"), Some(ROOT_PARTITION)),
            "/index/scratch/__root__/*.parquet"
        );
    }

    #[test]
    fn test_index_completion_warning() {
        let dir = tempfile::TempDir::new().unwrap();
        let index = dir.path();

        // No marker: a reader should say so, naming the index and the marker.
        let warning = index_completion_warning(index).expect("markerless index must warn");
        assert!(warning.contains(&index.display().to_string()));
        assert!(warning.contains(COMPLETION_MARKER));

        // Marker present: silence.
        std::fs::write(index.join(COMPLETION_MARKER), "xdu=test\n").unwrap();
        assert_eq!(index_completion_warning(index), None);

        // A marker recording tolerated errors is present but still not trustworthy: warn, and
        // do it without the substring the markerless case owns so the two stay distinguishable.
        std::fs::write(index.join(COMPLETION_MARKER), "xdu=test\nerrors=2\n").unwrap();
        let warning = index_completion_warning(index).expect("a tolerated-error index must warn");
        assert!(warning.contains(&index.display().to_string()));
        assert!(warning.contains("2 tolerated error(s)"));
        assert!(warning.contains("--allow-errors"));
        assert!(
            !warning.contains("completion marker"),
            "must stay distinguishable from the markerless warning: {warning}"
        );

        // A clean run records zero, which is nothing to report.
        std::fs::write(index.join(COMPLETION_MARKER), "xdu=test\nerrors=0\n").unwrap();
        assert_eq!(index_completion_warning(index), None);

        // Something of that name that is not a file still attests presence, and offers no
        // body worth reading.
        std::fs::remove_file(index.join(COMPLETION_MARKER)).unwrap();
        std::fs::create_dir(index.join(COMPLETION_MARKER)).unwrap();
        assert_eq!(index_completion_warning(index), None);
    }

    #[test]
    fn test_completion_marker_errors_reads_the_writers_body() {
        // Writer and reader pinned by one test: if `completion_marker_contents` ever renames
        // or reformats the key, this fails loudly instead of the readers going quietly silent.
        let body = crawl::completion_marker_contents(
            &crawl::CrawlStats {
                errors: 3,
                ..Default::default()
            },
            1_700_000_000,
        );
        // The writer stamps the format the gate checks: pin both ends together so a
        // renamed key fails here instead of refusing every index at query time.
        assert_eq!(completion_marker_errors(&body), Some(3));
        assert_eq!(completion_marker_format(&body), Some(INDEX_FORMAT_VERSION));
        assert_eq!(
            completion_marker_errors(&crawl::completion_marker_contents(
                &crawl::CrawlStats::default(),
                1_700_000_000
            )),
            Some(0)
        );

        // Nothing to say: no key, no body, or a format this version does not understand.
        assert_eq!(completion_marker_errors(""), None);
        assert_eq!(completion_marker_errors("xdu=0.4.1\nfiles=10\n"), None);
        assert_eq!(completion_marker_errors("errors=garbage\n"), None);
        assert_eq!(completion_marker_errors("errors=-1\n"), None);
        assert_eq!(completion_marker_errors("errors=\n"), None);
        assert_eq!(completion_marker_errors("no separator here\n"), None);

        // Stray whitespace, a missing trailing newline, and CRLF all still parse.
        assert_eq!(completion_marker_errors("errors=4"), Some(4));
        assert_eq!(completion_marker_errors("errors= 5 \n"), Some(5));
        assert_eq!(completion_marker_errors("files=1\r\nerrors=6\r\n"), Some(6));

        // The first key trimming to `errors` decides the answer.
        assert_eq!(completion_marker_errors("errors=1\nerrors=9\n"), Some(1));
    }

    #[test]
    fn test_completion_marker_format_parses_the_writers_key() {
        // The tool release beside it is not a format: a crate version must never read as
        // compatibility, or every upgrade would refuse every readable index.
        assert_eq!(completion_marker_format("xdu=7\n"), None);
        assert_eq!(completion_marker_format(""), None);
        assert_eq!(completion_marker_format("xdu=0.4.1\nfiles=10\n"), None);
        assert_eq!(completion_marker_format("format=garbage\n"), None);
        assert_eq!(completion_marker_format("format=-1\n"), None);
        assert_eq!(completion_marker_format("format=\n"), None);
        assert_eq!(completion_marker_format("format=1.5\n"), None);
        assert_eq!(completion_marker_format("no separator here\n"), None);

        // Stray whitespace, a missing trailing newline, and CRLF all still parse.
        assert_eq!(completion_marker_format("format=1"), Some(1));
        assert_eq!(completion_marker_format("format= 2 \n"), Some(2));
        assert_eq!(completion_marker_format("files=1\r\nformat=3\r\n"), Some(3));

        // The first key trimming to `format` decides the answer.
        assert_eq!(completion_marker_format("format=4\nformat=9\n"), Some(4));
    }

    #[test]
    fn test_index_version_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let index = dir.path();

        // No marker answers nothing about the layout: refuse, directing a re-index.
        let error = index_version_error(index).expect("markerless index must refuse");
        assert!(error.contains("no index format version"));
        assert!(error.contains("re-index"));

        // A pre-versioning marker is versionless, however clean its run was: same refusal.
        std::fs::write(index.join(COMPLETION_MARKER), "xdu=test\nerrors=0\n").unwrap();
        let error = index_version_error(index).expect("versionless index must refuse");
        assert!(error.contains("no index format version"));
        assert!(error.contains("re-index"));

        // Garbage where the version belongs is not a version.
        std::fs::write(index.join(COMPLETION_MARKER), "xdu=test\nformat=new\n").unwrap();
        assert!(index_version_error(index).is_some());

        // A version this build does not understand names both sides of the mismatch.
        std::fs::write(index.join(COMPLETION_MARKER), "xdu=test\nformat=999\n").unwrap();
        let error = index_version_error(index).expect("future version must refuse");
        assert!(error.contains("999"));
        assert!(error.contains(&INDEX_FORMAT_VERSION.to_string()));
        assert!(error.contains("re-index"));

        // What the writer emits is accepted, tolerated errors and all — the version gate
        // passes and the completeness warning stays that function's own job.
        let body = crawl::completion_marker_contents(
            &crawl::CrawlStats {
                errors: 2,
                ..Default::default()
            },
            1_700_000_000,
        );
        std::fs::write(index.join(COMPLETION_MARKER), &body).unwrap();
        assert_eq!(index_version_error(index), None);

        // Something of that name that is not a file cannot vouch for the layout either.
        std::fs::remove_file(index.join(COMPLETION_MARKER)).unwrap();
        std::fs::create_dir(index.join(COMPLETION_MARKER)).unwrap();
        assert!(index_version_error(index).is_some());
    }

    #[test]
    fn test_index_completion_warning_does_not_block_on_a_fifo_marker() {
        use std::os::unix::ffi::OsStrExt;
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempfile::TempDir::new().unwrap();
        let marker = dir.path().join(COMPLETION_MARKER);

        // Opening a FIFO read-only blocks until a writer appears, so reading the body without
        // first checking the file type would hang every reader forever. An index directory on
        // shared scratch is routinely group-writable, so this is reachable, not theoretical.
        let c_path = std::ffi::CString::new(marker.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) }, 0);

        let index = dir.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(index_completion_warning(&index));
        });

        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => assert_eq!(
                result, None,
                "a FIFO marker attests presence and yields no body"
            ),
            Err(_) => panic!(
                "index_completion_warning blocked on a FIFO marker — the file-type guard is gone"
            ),
        }
    }

    fn s3_locator(raw: &str) -> S3Locator {
        S3Locator::parse(std::ffi::OsStr::new(raw)).unwrap()
    }

    #[test]
    fn test_index_glob_s3_shares_the_layout() {
        let index = s3_locator("s3://bucket/some/prefix/");
        assert_eq!(
            index_glob_s3(&index, None),
            "s3://bucket/some/prefix/*/*.parquet"
        );
        assert_eq!(
            index_glob_s3(&index, Some(ROOT_PARTITION)),
            "s3://bucket/some/prefix/__root__/*.parquet"
        );
        // A bucket-root index has no empty segment before the partition.
        assert_eq!(
            index_glob_s3(&s3_locator("s3://bucket"), Some("alice")),
            "s3://bucket/alice/*.parquet"
        );
    }

    #[test]
    fn test_parquet_source_keeps_local_sql_byte_for_byte() {
        // The exact text every local reader statement carried before S3 indices existed, with
        // Hive detection turned off ahead of any `filename=true`.
        let local = IndexLocator::Local(PathBuf::from("/index/scratch"));
        assert_eq!(
            parquet_source(&local, None, false),
            (
                "read_parquet('/index/scratch/*/*.parquet', hive_partitioning = false)".to_string(),
                vec![]
            )
        );
        assert_eq!(
            parquet_source(&local, Some("alice"), true),
            (
                "read_parquet('/index/scratch/alice/*.parquet', hive_partitioning = false, filename=true)"
                    .to_string(),
                vec![]
            )
        );
    }

    #[test]
    fn test_parquet_source_binds_the_s3_glob() {
        // A quote in the prefix or the partition would end a literal; bound, it never reaches
        // the text.
        let index = IndexLocator::S3(s3_locator("s3://bucket/it's"));
        assert_eq!(
            parquet_source(&index, None, false),
            (
                "read_parquet(?, hive_partitioning = false)".to_string(),
                vec!["s3://bucket/it's/*/*.parquet".to_string()]
            )
        );
        let (sql, params) = parquet_source(&index, Some("o'brien"), true);
        assert_eq!(
            sql,
            "read_parquet(?, hive_partitioning = false, filename=true)"
        );
        assert_eq!(
            params,
            vec!["s3://bucket/it's/o'brien/*.parquet".to_string()]
        );
        assert!(!sql.contains('\''));
    }

    #[test]
    fn test_classify_s3_text_names_each_measured_cause() {
        assert_eq!(
            classify_s3_text(
                "IO Error: SSL server verification failed error for HTTP GET to \
                 '/b/?encoding-type=url&list-type=2&prefix=p%2F'"
            ),
            S3Cause::Untrusted
        );
        assert_eq!(
            classify_s3_text(
                "IO Error: Could not establish connection error for HTTP GET to '/b/?list-type=2'"
            ),
            S3Cause::Unreachable
        );
        assert_eq!(
            classify_s3_text(
                "HTTP Error: HTTP GET error reading 's3://b/p/b/?list-type=2' in region \
                 'us-east-1' (HTTP 403 Forbidden)"
            ),
            S3Cause::Forbidden
        );
        assert_eq!(
            classify_s3_text(
                "HTTP Error: HTTP GET error on 'http://h/b/p/.xdu-complete' (HTTP 403)"
            ),
            S3Cause::Forbidden
        );
        assert_eq!(
            classify_s3_text(
                "HTTP Error: HTTP GET error reading 's3://b/p/b/?list-type=2' in region \
                 'us-east-1' (HTTP 404 Not Found)"
            ),
            S3Cause::NotFound
        );
        assert_eq!(
            classify_s3_text(
                "HTTP Error: Unable to connect to URL \"http://h/b/p/.xdu-complete\": 404 (Not \
                 Found)."
            ),
            S3Cause::NotFound
        );
        let other = "HTTP Error: HTTP GET error on 'http://h/b/k' (HTTP 500)";
        assert_eq!(classify_s3_text(other), S3Cause::Other(other.to_string()));
    }

    #[test]
    fn test_marker_gates_refuse_an_oversized_s3_marker_naming_the_cap() {
        let marker = MarkerRead::Oversized(MARKER_READ_LIMIT + 1);
        let error = marker_version_error(&marker, "s3://bucket/idx").unwrap();
        assert!(error.contains("s3://bucket/idx"), "{error}");
        assert!(error.contains(&MARKER_READ_LIMIT.to_string()), "{error}");
        assert!(
            error.contains(&(MARKER_READ_LIMIT + 1).to_string()),
            "{error}"
        );
        // An unread marker adds no detail to warn about, as locally.
        assert_eq!(marker_completion_warning(&marker, "s3://bucket/idx"), None);
    }

    #[test]
    fn test_marker_gates_read_a_body_as_the_local_gates_do() {
        let dir = tempfile::TempDir::new().unwrap();
        let index = dir.path();
        let body = "xdu=0\nerrors=2\nformat=2\n";
        std::fs::write(index.join(COMPLETION_MARKER), body).unwrap();
        let shown = index.display().to_string();
        let read = MarkerRead::Body(body.to_string());
        assert_eq!(
            marker_version_error(&read, &shown),
            index_version_error(index)
        );
        assert_eq!(
            marker_completion_warning(&read, &shown),
            index_completion_warning(index)
        );
        assert!(marker_completion_warning(&read, &shown).is_some());
        assert_eq!(marker_version_error(&read, &shown), None);
    }

    #[test]
    fn test_index_completion_warning_ignores_an_oversized_marker() {
        let dir = tempfile::TempDir::new().unwrap();
        let index = dir.path();

        // This body does record errors, but a reader must not pull 64 KiB+ into memory to
        // find that out — the size guard wins over the key.
        let mut body = String::from("errors=7\n");
        body.push_str(&"x".repeat(MARKER_READ_LIMIT as usize + 1));
        std::fs::write(index.join(COMPLETION_MARKER), &body).unwrap();

        assert_eq!(index_completion_warning(index), None);
    }
}
