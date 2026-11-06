//! The index-build crawl hot path, lifted out of the `xdu` binary so its
//! decision/ordering logic and its Parquet-finalization logic are unit-testable.
//!
//! Only the crawler pulls this in; the reader tools (`xdu-find`/`xdu-view`/`xdu-rm`)
//! never touch it. The pieces here are pure or near-pure: work-queue construction,
//! per-file record building, and the `PartitionBuffer` that accumulates records and
//! hands each encoded chunk to an `IndexSink`. The concurrency scaffold (shared rayon
//! pool, driver threads, `thread::scope` error propagation) stays in `src/bin/xdu.rs`.

use std::borrow::Cow;
use std::collections::{HashSet, VecDeque};
use std::fs::{self, Metadata};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow::array::{Int64Builder, StringBuilder};
use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::SizeMode;

/// The index-layout names, which readers share — re-exported so crawl-side code reads
/// in one place.
///
/// `ROOT_PARTITION` is reserved: a real top-level subdirectory of that name would collide
/// with the synthetic depth-1 partition holding loose root files, so `build_work_queue`
/// rejects one rather than letting two work items clobber the same chunk ids.
///
/// `COMPLETION_MARKER` exists because per-chunk `partial`→`rename` finalization is atomic
/// for one file but cannot express whether the *run* finished: when one driver fails, the
/// partitions that already succeeded remain on disk as real `.parquet` chunks,
/// indistinguishable from a complete index. The marker is removed once a run's pre-flight
/// has passed and it is about to write, and written only when it succeeds, so its presence
/// attests to the whole run.
///
/// `RESERVED_INDEX_NAMES` pairs both names with what claims them: it is the list
/// `build_work_queue` rejects a top-level source directory against, so every name the index
/// root claims is guarded in both directions.
pub use crate::{COMPLETION_MARKER, INDEX_FORMAT_VERSION, RESERVED_INDEX_NAMES, ROOT_PARTITION};

/// Location of the completion marker for an index directory.
pub fn completion_marker_path(index: &Path) -> PathBuf {
    index.join(COMPLETION_MARKER)
}

/// Drop any existing completion marker once a run's pre-flight has passed and it is
/// about to write.
///
/// A run that is rewriting the index must never carry the previous run's attestation: if
/// this one dies before finishing, the index is left markerless. Call this *after* the
/// last check that can still reject the run and *before* the first write, so a rejected
/// run leaves an already-complete index still attested. A missing marker is not an
/// error — most runs start against a fresh or already-markerless directory.
pub fn clear_completion_marker(index: &Path) -> Result<()> {
    let path = completion_marker_path(index);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to remove stale completion marker: {}",
                path.display()
            )
        }),
    }
}

/// The marker body: the crawler version and this run's totals, one `key=value` per
/// line, plus the index format version readers gate on. The counts are recorded so a
/// tolerated-error run (`--allow-errors`) still says how much it skipped.
pub fn completion_marker_contents(stats: &CrawlStats, completed_at: u64) -> String {
    format!(
        "xdu={}\ncompleted_at={}\nfiles={}\nbytes={}\nvanished={}\nerrors={}\nlossy_paths={}\nformat={}\n",
        env!("CARGO_PKG_VERSION"),
        completed_at,
        stats.files,
        stats.bytes,
        stats.vanished,
        stats.errors,
        stats.lossy_paths,
        INDEX_FORMAT_VERSION,
    )
}

/// Write the completion marker. Only ever called on a run's success path.
pub fn write_completion_marker(index: &Path, contents: &str) -> Result<()> {
    let path = completion_marker_path(index);
    fs::write(&path, contents)
        .with_context(|| format!("Failed to write completion marker: {}", path.display()))
}

/// Where a run's chunks and its completion marker go.
///
/// The crawl drives every destination through these four steps, in the order the
/// completion-marker invariant fixes: `clear_marker` once every check that can reject the
/// run has passed, `put_chunk` as each chunk is encoded, `finalize_partition` when a
/// partition's walk ends, and `write_marker` only on the success path.
///
/// `LocalSink` is the local directory, written exactly as before; `crate::s3::S3Sink` is an
/// S3 prefix. A sink is shared by every driver thread.
pub trait IndexSink: Send + Sync {
    /// Check the destination after every source-side check and before anything is written,
    /// so a destination this run cannot use rejects it with any existing marker intact.
    fn preflight(&self) -> Result<()> {
        Ok(())
    }

    /// Refuse a top-level source directory this destination cannot hold under its own name.
    /// Called for every top-level entry before the marker is cleared; a local directory
    /// holds any name, so the default accepts everything.
    fn check_top_level(&self, _entries: &[TopEntry], _top_dir: &Path) -> Result<()> {
        Ok(())
    }

    /// Remove the previous run's attestation. A missing marker is not an error.
    fn clear_marker(&self) -> Result<()>;

    /// Store chunk `id` of `partition`. The bytes are a complete Parquet file, moved in so a
    /// sink that sends them on needs no copy.
    fn put_chunk(&self, partition: &str, id: usize, bytes: Vec<u8>) -> Result<()>;

    /// Make the `count` chunks this run stored for `partition` the partition's whole
    /// contents: publish any that are not yet visible, and remove the higher-numbered chunks
    /// a larger earlier run left. Returns how many stale chunks were removed.
    fn finalize_partition(&self, partition: &str, count: usize) -> Result<usize>;

    /// Write the completion marker. Only ever called on a run's success path.
    fn write_marker(&self, contents: &str) -> Result<()>;

    /// Where the completion marker lives, for the record that names it.
    fn marker_location(&self) -> String;
}

/// An index in a local directory: each chunk written as `NNNNNN.parquet.partial`, renamed
/// into place within the same directory when its partition finalizes, the stale tail pruned
/// up to the first gap, and the marker a file at the index root.
pub struct LocalSink {
    outdir: PathBuf,
}

impl LocalSink {
    pub fn new(outdir: PathBuf) -> Self {
        Self { outdir }
    }
}

impl IndexSink for LocalSink {
    fn clear_marker(&self) -> Result<()> {
        clear_completion_marker(&self.outdir)
    }

    fn put_chunk(&self, partition: &str, id: usize, bytes: Vec<u8>) -> Result<()> {
        let partition_dir = self.outdir.join(partition);
        fs::create_dir_all(&partition_dir).with_context(|| {
            format!(
                "Failed to create partition dir: {}",
                partition_dir.display()
            )
        })?;

        // Write to .partial file first; finalize renames it atomically.
        let partial_path = partition_dir.join(chunk_partial_name(id));
        fs::write(&partial_path, bytes)
            .with_context(|| format!("Failed to create file: {}", partial_path.display()))
    }

    fn finalize_partition(&self, partition: &str, count: usize) -> Result<usize> {
        let partition_dir = self.outdir.join(partition);

        // Rename all .partial files to .parquet (atomic on POSIX).
        for chunk_id in 0..count {
            let partial_path = partition_dir.join(chunk_partial_name(chunk_id));
            let final_path = partition_dir.join(chunk_final_name(chunk_id));
            fs::rename(&partial_path, &final_path).with_context(|| {
                format!(
                    "Failed to rename {} to {}",
                    partial_path.display(),
                    final_path.display()
                )
            })?;
        }

        // Prune any stale chunks beyond what we just wrote (from a prior larger run).
        let mut pruned = 0;
        for chunk_id in count.. {
            let stale_path = partition_dir.join(chunk_final_name(chunk_id));
            if stale_path.exists() {
                fs::remove_file(&stale_path).with_context(|| {
                    format!("Failed to remove stale chunk: {}", stale_path.display())
                })?;
                pruned += 1;
            } else {
                break; // No more consecutive chunks
            }
        }

        Ok(pruned)
    }

    fn write_marker(&self, contents: &str) -> Result<()> {
        write_completion_marker(&self.outdir, contents)
    }

    fn marker_location(&self) -> String {
        completion_marker_path(&self.outdir).display().to_string()
    }
}

/// The chunk filename that is written first, before atomic finalization.
///
/// Zero-padded sequential ids give a deterministic, gap-free chunk sequence that
/// `finalize` can prune against.
pub fn chunk_partial_name(id: usize) -> String {
    format!("{:06}.parquet.partial", id)
}

/// The finalized chunk filename a reader globs (`*/*.parquet`).
pub fn chunk_final_name(id: usize) -> String {
    format!("{:06}.parquet", id)
}

/// Encode one chunk's rows as a complete Parquet file into `sink`, returning the sink.
///
/// The one place chunk bytes are produced: every chunk of every index is written with these
/// properties, whatever it is written to, so two copies of the encoder cannot drift into two
/// file formats. Rows take the schema of `batch`, which callers build from `get_schema()`.
pub fn encode_chunk<W: Write + Send>(sink: W, batch: &RecordBatch) -> Result<W> {
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(sink, batch.schema(), Some(props))?;
    writer.write(batch)?;
    Ok(writer.into_inner()?)
}

/// The seven integer index columns derived from a file's metadata, under the chosen
/// size mode: size, uid, gid, permission bits, atime, mtime, ctime — schema order,
/// so the call site unpacks them straight into `PartitionBuffer::add`.
///
/// The `MetadataExt` reads (`blocks()`×512 for disk usage, `atime()`, `uid()`,
/// `gid()`, `mode()`, `mtime()`, `ctime()`) are Unix-only. The caller performs the
/// `stat` syscall and passes the borrowed `Metadata` in. The stored mode is masked to
/// the permission bits; the file-type bits are constant (only regular files are
/// indexed) and would only force every reader to mask them back off.
pub fn file_measurements(
    metadata: &Metadata,
    size_mode: SizeMode,
) -> (i64, i64, i64, i64, i64, i64, i64) {
    let disk_usage = metadata.blocks() * 512;
    let file_len = metadata.len();
    let file_size = size_mode.calculate(disk_usage, file_len);

    (
        file_size as i64,
        metadata.uid() as i64,
        metadata.gid() as i64,
        (metadata.mode() & 0o7777) as i64,
        metadata.atime(),
        metadata.mtime(),
        metadata.ctime(),
    )
}

/// Convert a path to the index's UTF-8 `path` column, reporting whether bytes were lost.
///
/// The schema's `path` column is UTF-8, so a filename carrying invalid bytes is stored
/// with U+FFFD replacements: the row no longer names a real file and cannot be unlinked
/// by `xdu-rm`. The caller counts and reports those rows — storing the raw bytes would
/// require an index-format change. A valid UTF-8 path borrows, so the common case does
/// not allocate at all.
pub fn lossy_path(path: &Path) -> (Cow<'_, str>, bool) {
    match path.to_string_lossy() {
        Cow::Borrowed(valid) => (Cow::Borrowed(valid), false),
        Cow::Owned(replaced) => (Cow::Owned(replaced), true),
    }
}

/// A top-level entry under the indexed root, classified by the crawler's `read_dir`
/// pass. The directory I/O lives in the bin; `build_work_queue` is pure over these
/// so the ordering/classification decision can be tested without a filesystem.
///
/// `path` carries the real bytes from `read_dir` (so a non-UTF-8 directory name is
/// still walked correctly), while `name` is the lossy string used as the partition
/// key and for `--partition` filtering — mirroring the original inline logic.
pub struct TopEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_symlink: bool,
}

/// A unit of work for a driver thread: one partition to crawl.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkItem {
    pub path: PathBuf,
    pub partition: String,
    pub max_depth: Option<usize>,
}

/// Statistics returned from a crawl operation.
///
/// `vanished` counts benign skips (a file that disappeared between walk and stat —
/// an `ENOENT` race, common on a live filesystem); `errors` counts hard failures
/// (permission/I/O, or a directory read that hid a whole subtree) that make the run
/// exit non-zero unless `--allow-errors` is set; `lossy_paths` counts rows whose
/// path was stored with U+FFFD replacements and so cannot round-trip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CrawlStats {
    pub files: u64,
    pub bytes: u64,
    pub pruned: usize,
    pub vanished: u64,
    pub errors: u64,
    pub lossy_paths: u64,
}

impl CrawlStats {
    /// Fold another unit's totals into this one (associative aggregation).
    pub fn merge(&mut self, other: &CrawlStats) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.pruned += other.pruned;
        self.vanished += other.vanished;
        self.errors += other.errors;
        self.lossy_paths += other.lossy_paths;
    }
}

/// How a per-entry crawl error should be handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryError {
    /// The entry vanished between walk and stat (`ENOENT` race) — benign; skip and
    /// keep the run's exit status clean.
    Vanished,
    /// A hard error (permission denied, I/O, a directory read that hid a subtree, or a
    /// non-I/O walk error such as a symlink loop) — report it and fail the run.
    Hard,
}

/// Classify an I/O error encountered while walking or stat-ing an entry.
///
/// `kind` is `None` when a walk error carries no underlying `io::Error` (a symlink
/// loop or a busy thread-pool); those are treated as hard. Only `NotFound` (the file
/// raced away) is benign — every other kind hides data and must fail the run.
pub fn classify_io_error(kind: Option<std::io::ErrorKind>) -> EntryError {
    match kind {
        Some(std::io::ErrorKind::NotFound) => EntryError::Vanished,
        _ => EntryError::Hard,
    }
}

/// Turn the classified top-level entries into an ordered work queue.
///
/// Pure decision/ordering: directories become partition `WorkItem`s (subject to the
/// `--partition` filter) sorted ascending for deterministic output; a single depth-1
/// `__root__` item is pushed **first** iff any loose top-level file exists. An empty
/// result is an error — there is nothing to index.
///
/// A directory whose name appears in `RESERVED_INDEX_NAMES` is rejected before the
/// `--partition` filter is consulted: it would be written over an entry the index root
/// already owns.
///
/// The root trigger is `is_file()` only: the walk excludes symlinks, so a root whose
/// only loose entries are symlinks would otherwise spawn a `__root__` partition that
/// indexes nothing.
pub fn build_work_queue(
    entries: Vec<TopEntry>,
    top_dir: &Path,
    partition_filter: Option<&HashSet<String>>,
) -> Result<VecDeque<WorkItem>> {
    let mut partition_items: Vec<WorkItem> = Vec::new();
    let mut has_root_files = false;

    for entry in entries {
        if entry.is_dir {
            // A subdirectory whose name the index root already claims would be written
            // as that entry: over the synthetic loose-file partition (both start at
            // chunk id 0, so their chunks overwrite each other and each one's finalize
            // prunes the other's tail), or over the completion marker (a directory at
            // that path fails the attestation write and leaves every later run unable
            // to clear it, from any source tree). Reject instead of corrupting — the
            // check is unconditional because the collision is with what is already
            // on disk, not with what this run happens to select, and it iterates the
            // whole reserved list so a newly reserved name is covered by construction.
            if let Some((name, claimed_by)) = RESERVED_INDEX_NAMES
                .iter()
                .find(|(name, _)| *name == entry.name)
            {
                anyhow::bail!(
                    "top-level directory '{}' in {} uses a name reserved by the index \
                     layout for {}; rename it before indexing",
                    name,
                    top_dir.display(),
                    claimed_by
                );
            }
            if let Some(pf) = partition_filter
                && !pf.contains(&entry.name)
            {
                continue;
            }
            partition_items.push(WorkItem {
                path: entry.path,
                partition: entry.name,
                max_depth: None,
            });
        } else if entry.is_file {
            has_root_files = true;
        }
    }

    // Sort for deterministic output order.
    partition_items.sort_by(|a, b| a.partition.cmp(&b.partition));

    // Root files first (depth-limited), then partition subdirectories.
    let mut work_queue: VecDeque<WorkItem> = VecDeque::with_capacity(partition_items.len() + 1);

    if has_root_files {
        work_queue.push_back(WorkItem {
            path: top_dir.to_path_buf(),
            partition: ROOT_PARTITION.to_string(),
            max_depth: Some(1),
        });
    }
    for item in partition_items {
        work_queue.push_back(item);
    }

    if work_queue.is_empty() {
        anyhow::bail!("No partitions found in {}", top_dir.display());
    }

    // Two work items sharing a partition name would clobber each other's chunks. The
    // names come from distinct directory entries, but a name carrying invalid UTF-8
    // is lossily converted, so two different directories can still collapse onto one
    // key here — a real guard, not a redundant assertion.
    let mut seen: HashSet<&str> = HashSet::with_capacity(work_queue.len());
    for item in &work_queue {
        if !seen.insert(item.partition.as_str()) {
            anyhow::bail!(
                "two top-level directories in {} map to the same partition name '{}' \
                 (a non-UTF-8 name is stored lossily); rename one before indexing",
                top_dir.display(),
                item.partition
            );
        }
    }

    Ok(work_queue)
}

/// Typical bytes per path, used only to size the chunk's initial string buffer.
const ESTIMATED_PATH_BYTES: usize = 64;

/// Rows the builders reserve up front, capped well below a default `--buffsize`.
///
/// Reserving a whole chunk would allocate megabytes per partition even for a partition
/// holding a handful of files — and an index over one directory per user is mostly small
/// partitions. The builders grow geometrically instead, so a large partition reaches full
/// size in a few reallocations whose cost is nothing beside the `stat` per row.
const INITIAL_ROW_CAPACITY: usize = 8192;

/// One pre-sized Arrow builder per index column. A struct, not a tuple: eight
/// positional builders would invite an ordering slip every time one is touched.
struct ChunkBuilders {
    path: StringBuilder,
    size: Int64Builder,
    uid: Int64Builder,
    gid: Int64Builder,
    mode: Int64Builder,
    atime: Int64Builder,
    mtime: Int64Builder,
    ctime: Int64Builder,
}

impl ChunkBuilders {
    /// Builders sized for a first batch of rows; they grow from there as needed.
    fn new(buffsize: usize) -> Self {
        let rows = buffsize.min(INITIAL_ROW_CAPACITY);
        Self {
            path: StringBuilder::with_capacity(rows, rows.saturating_mul(ESTIMATED_PATH_BYTES)),
            size: Int64Builder::with_capacity(rows),
            uid: Int64Builder::with_capacity(rows),
            gid: Int64Builder::with_capacity(rows),
            mode: Int64Builder::with_capacity(rows),
            atime: Int64Builder::with_capacity(rows),
            mtime: Int64Builder::with_capacity(rows),
            ctime: Int64Builder::with_capacity(rows),
        }
    }
}

/// Per-partition buffer that accumulates records and flushes them to its sink as Parquet.
///
/// Records are appended straight into the Arrow builders as they arrive, so a path's
/// bytes are copied once — into the column that gets written — rather than into an
/// intermediate row struct first. `flush` then only has to `finish` the builders, encode
/// the chunk once into memory and hand the bytes to the sink.
pub struct PartitionBuffer {
    partition: String,
    sink: Arc<dyn IndexSink>,
    builders: ChunkBuilders,
    /// Rows appended since the last flush (the builders do not expose a row count).
    buffered: usize,
    buffsize: usize,
    chunk_counter: usize,
    schema: Arc<Schema>,
    /// Track statistics for this partition.
    pub file_count: u64,
    pub byte_count: u64,
}

impl PartitionBuffer {
    pub fn new(
        partition: String,
        sink: Arc<dyn IndexSink>,
        buffsize: usize,
        schema: Arc<Schema>,
    ) -> Self {
        Self {
            partition,
            sink,
            builders: ChunkBuilders::new(buffsize),
            buffered: 0,
            buffsize,
            chunk_counter: 0,
            schema,
            file_count: 0,
            byte_count: 0,
        }
    }

    /// Append one indexed file. `size` is already resolved under the run's `SizeMode`.
    ///
    /// Eight positional values are the whole row in schema order; grouping them
    /// would reintroduce the intermediate row type the builders exist to avoid.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        path: &str,
        size: i64,
        uid: i64,
        gid: i64,
        mode: i64,
        atime: i64,
        mtime: i64,
        ctime: i64,
    ) -> Result<()> {
        self.file_count += 1;
        self.byte_count += size as u64;

        self.builders.path.append_value(path);
        self.builders.size.append_value(size);
        self.builders.uid.append_value(uid);
        self.builders.gid.append_value(gid);
        self.builders.mode.append_value(mode);
        self.builders.atime.append_value(atime);
        self.builders.mtime.append_value(mtime);
        self.builders.ctime.append_value(ctime);
        self.buffered += 1;

        if self.buffered >= self.buffsize {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        if self.buffered == 0 {
            return Ok(());
        }

        let chunk_id = self.chunk_counter;
        self.chunk_counter += 1;

        // `finish` hands over the accumulated buffers and leaves the builders empty, so
        // the next chunk starts from a fresh pre-sized set.
        let schema = self.schema.clone();
        let mut builders = std::mem::replace(&mut self.builders, ChunkBuilders::new(self.buffsize));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(builders.path.finish()),
                Arc::new(builders.size.finish()),
                Arc::new(builders.uid.finish()),
                Arc::new(builders.gid.finish()),
                Arc::new(builders.mode.finish()),
                Arc::new(builders.atime.finish()),
                Arc::new(builders.mtime.finish()),
                Arc::new(builders.ctime.finish()),
            ],
        )?;
        self.buffered = 0;

        // One encode, into memory; the sink decides how the bytes become a chunk.
        let bytes = encode_chunk(Vec::new(), &batch)?;
        self.sink.put_chunk(&self.partition, chunk_id, bytes)
    }

    /// Finalize every chunk this buffer flushed and prune the stale tail, through the sink.
    pub fn finalize(&self) -> Result<usize> {
        self.sink
            .finalize_partition(&self.partition, self.chunk_counter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::get_schema;
    use std::fs::File;
    use std::io::Write;

    fn local_buffer(outdir: &Path, buffsize: usize) -> PartitionBuffer {
        let sink: Arc<dyn IndexSink> = Arc::new(LocalSink::new(outdir.to_path_buf()));
        PartitionBuffer::new("part".to_string(), sink, buffsize, get_schema())
    }

    /// One expected row in schema order: path plus the seven integer columns.
    type TestRow = (String, i64, i64, i64, i64, i64, i64, i64);

    fn top_dir(name: &str, is_dir: bool) -> TopEntry {
        TopEntry {
            path: Path::new("/data").join(name),
            name: name.to_string(),
            is_dir,
            is_file: !is_dir,
            is_symlink: false,
        }
    }

    // ---- chunk name helpers -------------------------------------------------

    #[test]
    fn test_chunk_name_helpers() {
        assert_eq!(chunk_partial_name(0), "000000.parquet.partial");
        assert_eq!(chunk_partial_name(42), "000042.parquet.partial");
        assert_eq!(chunk_final_name(0), "000000.parquet");
        assert_eq!(chunk_final_name(123456), "123456.parquet");
    }

    // ---- CrawlStats fold ----------------------------------------------------

    #[test]
    fn test_crawl_stats_merge_folds_totals() {
        let mut total = CrawlStats::default();
        assert_eq!(total, CrawlStats::default());

        total.merge(&CrawlStats {
            files: 3,
            bytes: 300,
            pruned: 1,
            vanished: 2,
            errors: 1,
            lossy_paths: 1,
        });
        total.merge(&CrawlStats {
            files: 2,
            bytes: 200,
            pruned: 0,
            vanished: 0,
            errors: 0,
            lossy_paths: 0,
        });
        total.merge(&CrawlStats {
            files: 5,
            bytes: 500,
            pruned: 4,
            vanished: 3,
            errors: 2,
            lossy_paths: 6,
        });

        assert_eq!(
            total,
            CrawlStats {
                files: 10,
                bytes: 1000,
                pruned: 5,
                vanished: 5,
                errors: 3,
                lossy_paths: 7,
            }
        );
    }

    #[test]
    fn test_classify_io_error() {
        use std::io::ErrorKind;
        assert_eq!(
            classify_io_error(Some(ErrorKind::NotFound)),
            EntryError::Vanished
        );
        assert_eq!(
            classify_io_error(Some(ErrorKind::PermissionDenied)),
            EntryError::Hard
        );
        assert_eq!(classify_io_error(Some(ErrorKind::Other)), EntryError::Hard);
        // A walk error with no underlying io::Error (loop / busy pool) is hard.
        assert_eq!(classify_io_error(None), EntryError::Hard);
    }

    // ---- build_work_queue ---------------------------------------------------

    #[test]
    fn test_build_work_queue_root_first_and_sorted() {
        let entries = vec![
            top_dir("bravo", true),
            top_dir("alpha", true),
            top_dir("loose.txt", false), // a loose top-level regular file
        ];
        let base = Path::new("/data");
        let queue = build_work_queue(entries, base, None).unwrap();

        let names: Vec<&str> = queue.iter().map(|w| w.partition.as_str()).collect();
        assert_eq!(names, vec![ROOT_PARTITION, "alpha", "bravo"]);

        // __root__ is depth-1; real partitions are unbounded depth.
        assert_eq!(queue[0].max_depth, Some(1));
        assert_eq!(queue[0].path, base.to_path_buf());
        assert_eq!(queue[1].max_depth, None);
        assert_eq!(queue[1].path, Path::new("/data/alpha"));
        assert_eq!(queue[2].max_depth, None);
    }

    #[test]
    fn test_build_work_queue_no_root_item_without_loose_files() {
        let entries = vec![top_dir("alpha", true), top_dir("bravo", true)];
        let queue = build_work_queue(entries, Path::new("/data"), None).unwrap();
        let names: Vec<&str> = queue.iter().map(|w| w.partition.as_str()).collect();
        assert_eq!(names, vec!["alpha", "bravo"]);
    }

    #[test]
    fn test_build_work_queue_partition_filter_excludes() {
        let entries = vec![
            top_dir("alpha", true),
            top_dir("bravo", true),
            top_dir("charlie", true),
        ];
        let filter: HashSet<String> = ["bravo".to_string()].into_iter().collect();
        let queue = build_work_queue(entries, Path::new("/data"), Some(&filter)).unwrap();
        let names: Vec<&str> = queue.iter().map(|w| w.partition.as_str()).collect();
        assert_eq!(names, vec!["bravo"]);
    }

    #[test]
    fn test_build_work_queue_symlink_does_not_trigger_root() {
        let link = TopEntry {
            path: Path::new("/data/link").to_path_buf(),
            name: "link".to_string(),
            is_dir: false,
            is_file: false,
            is_symlink: true,
        };

        // Symlinks are never indexed, so a loose one must not spawn an empty
        // __root__ partition alongside the real ones.
        let queue =
            build_work_queue(vec![top_dir("alpha", true), link], Path::new("/data"), None).unwrap();
        let names: Vec<&str> = queue.iter().map(|w| w.partition.as_str()).collect();
        assert_eq!(names, vec!["alpha"]);

        // A root holding nothing but symlinks has nothing to index at all.
        let only_link = TopEntry {
            path: Path::new("/data/link").to_path_buf(),
            name: "link".to_string(),
            is_dir: false,
            is_file: false,
            is_symlink: true,
        };
        let err = build_work_queue(vec![only_link], Path::new("/data"), None).unwrap_err();
        assert!(err.to_string().contains("No partitions found"));
    }

    #[test]
    fn test_build_work_queue_empty_errors() {
        let err = build_work_queue(Vec::new(), Path::new("/data"), None).unwrap_err();
        assert!(err.to_string().contains("No partitions found"));
    }

    #[test]
    fn test_build_work_queue_rejects_every_reserved_index_name() {
        // Driven from the list itself, so a name reserved later is covered here without
        // this test being remembered — which is the failure the list exists to prevent.
        assert!(
            RESERVED_INDEX_NAMES.len() >= 2,
            "both the loose-file partition and the completion marker are reserved"
        );

        for (reserved, claimed_by) in RESERVED_INDEX_NAMES {
            let entries = vec![
                top_dir("alpha", true),
                top_dir(reserved, true),
                top_dir("loose.txt", false),
            ];
            let err = build_work_queue(entries, Path::new("/data"), None).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains(reserved), "message should name it: {msg}");
            assert!(
                msg.contains("reserved") && msg.contains(claimed_by),
                "message should explain what it collides with: {msg}"
            );

            // It is rejected even when no loose file would create the synthetic item
            // and even when a --partition filter would have excluded it: the collision
            // is with the on-disk layout, not with this run's selection.
            let filter: HashSet<String> = ["alpha".to_string()].into_iter().collect();
            let entries = vec![top_dir("alpha", true), top_dir(reserved, true)];
            assert!(build_work_queue(entries, Path::new("/data"), Some(&filter)).is_err());
        }

        // A reserved name borne by a loose *file* is not a collision: it becomes a row in
        // the __root__ partition, not a directory entry at the index root.
        let entries = vec![top_dir("alpha", true), top_dir(COMPLETION_MARKER, false)];
        let queue = build_work_queue(entries, Path::new("/data"), None).unwrap();
        let names: Vec<&str> = queue.iter().map(|i| i.partition.as_str()).collect();
        assert_eq!(names, vec![ROOT_PARTITION, "alpha"]);
    }

    #[test]
    fn test_build_work_queue_rejects_duplicate_partition_names() {
        // Two distinct directories whose non-UTF-8 names both convert to the same
        // lossy string would write into one partition directory.
        let dup = |path: &str| TopEntry {
            path: PathBuf::from(path),
            name: "a\u{FFFD}b".to_string(),
            is_dir: true,
            is_file: false,
            is_symlink: false,
        };
        let entries = vec![dup("/data/a\u{FFFD}b-one"), dup("/data/a\u{FFFD}b-two")];
        let err = build_work_queue(entries, Path::new("/data"), None).unwrap_err();
        assert!(
            err.to_string().contains("same partition name"),
            "message should explain the collapse: {err}"
        );
    }

    // ---- per-file measurement ----------------------------------------------

    #[test]
    fn test_file_measurements_size_modes_and_identity() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f.bin");
        {
            let mut f = File::create(&path).unwrap();
            f.write_all(&vec![b'x'; 5000]).unwrap();
        }
        let meta = fs::metadata(&path).unwrap();

        let (apparent, uid, gid, mode, atime, mtime, ctime) =
            file_measurements(&meta, SizeMode::ApparentSize);
        assert_eq!(apparent, 5000);
        assert_eq!(atime, meta.atime());
        assert_eq!(uid, meta.uid() as i64);
        assert_eq!(gid, meta.gid() as i64);
        assert_eq!(mode, (meta.mode() & 0o7777) as i64);
        assert_eq!(mtime, meta.mtime());
        assert_eq!(ctime, meta.ctime());

        let (disk, _, _, _, _, _, _) = file_measurements(&meta, SizeMode::DiskUsage);
        assert_eq!(disk, (meta.blocks() * 512) as i64);

        let (rounded, _, _, _, _, _, _) = file_measurements(&meta, SizeMode::BlockRounded(4096));
        assert_eq!(rounded, 8192); // 5000 rounds up to two 4096 blocks
    }

    #[test]
    fn test_lossy_path_flags_invalid_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // A valid path borrows: the common case does not allocate.
        let clean = Path::new("/data/alice/notes.txt");
        let (converted, lossy) = lossy_path(clean);
        assert!(!lossy);
        assert!(matches!(converted, Cow::Borrowed(_)));
        assert_eq!(converted, "/data/alice/notes.txt");

        // Invalid bytes are replaced, flagged, and therefore owned.
        let bad = PathBuf::from(OsStr::from_bytes(b"/data/bad\xffname"));
        let (converted, lossy) = lossy_path(&bad);
        assert!(lossy, "invalid UTF-8 bytes must be flagged");
        assert!(converted.contains('\u{FFFD}'));
    }

    // ---- completion marker --------------------------------------------------

    #[test]
    fn test_completion_marker_lifecycle() {
        let dir = tempfile::TempDir::new().unwrap();
        let index = dir.path();
        let marker = completion_marker_path(index);
        assert!(marker.ends_with(COMPLETION_MARKER));

        // Clearing an index that has no marker is a no-op, not an error.
        clear_completion_marker(index).unwrap();
        assert!(!marker.exists());

        let stats = CrawlStats {
            files: 7,
            bytes: 4096,
            pruned: 0,
            vanished: 1,
            errors: 2,
            lossy_paths: 3,
        };
        let contents = completion_marker_contents(&stats, 1_700_000_000);
        write_completion_marker(index, &contents).unwrap();
        assert!(marker.exists());

        let body = fs::read_to_string(&marker).unwrap();
        assert!(body.contains(&format!("xdu={}", env!("CARGO_PKG_VERSION"))));
        assert!(body.contains("completed_at=1700000000"));
        assert!(body.contains("files=7"));
        assert!(body.contains("bytes=4096"));
        assert!(body.contains("vanished=1"));
        assert!(body.contains("errors=2"));
        assert!(body.contains("lossy_paths=3"));
        assert!(body.contains(&format!("format={}", crate::INDEX_FORMAT_VERSION)));

        // A new run clears it again, leaving the index unattested until it finishes.
        clear_completion_marker(index).unwrap();
        assert!(!marker.exists());
    }

    // ---- PartitionBuffer::finalize -----------------------------------------

    #[test]
    fn test_finalize_renames_partials_and_prunes_stale_tail() {
        let dir = tempfile::TempDir::new().unwrap();
        let outdir = dir.path().to_path_buf();
        let mut buf = local_buffer(&outdir, 1);

        // buffsize 1 => each add flushes a chunk. Two records => 000000, 000001.
        buf.add("/part/a", 10, 1000, 1000, 0o644, 0, 50, 60)
            .unwrap();
        buf.add("/part/b", 20, 1000, 1000, 0o644, 0, 50, 60)
            .unwrap();

        let part_dir = outdir.join("part");
        // Seed a stale contiguous tail from a hypothetical prior larger run.
        for id in [2usize, 3] {
            File::create(part_dir.join(chunk_final_name(id))).unwrap();
        }

        let pruned = buf.finalize().unwrap();
        assert_eq!(pruned, 2);
        assert_eq!(buf.file_count, 2);

        assert!(part_dir.join(chunk_final_name(0)).exists());
        assert!(part_dir.join(chunk_final_name(1)).exists());
        assert!(!part_dir.join(chunk_final_name(2)).exists());
        assert!(!part_dir.join(chunk_final_name(3)).exists());
        // No .partial files survive a successful finalize.
        let leftovers = fs::read_dir(&part_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".partial"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn test_buffer_writes_every_row_across_chunk_boundaries() {
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

        let dir = tempfile::TempDir::new().unwrap();
        let outdir = dir.path().to_path_buf();
        // buffsize 2 over 5 rows exercises the auto-flush and the builder reset that
        // follows it, twice, plus a partial final chunk.
        let mut buf = local_buffer(&outdir, 2);

        let rows: Vec<TestRow> = (0..5)
            .map(|i| {
                (
                    format!("/part/f{i}"),
                    (i as i64 + 1) * 10,
                    1000 + i as i64,
                    2000 + i as i64,
                    0o644,
                    1000 + i as i64,
                    3000 + i as i64,
                    4000 + i as i64,
                )
            })
            .collect();
        for (path, size, uid, gid, mode, atime, mtime, ctime) in &rows {
            buf.add(path, *size, *uid, *gid, *mode, *atime, *mtime, *ctime)
                .unwrap();
        }
        buf.flush().unwrap();
        buf.finalize().unwrap();

        assert_eq!(buf.file_count, 5);
        assert_eq!(buf.byte_count, 10 + 20 + 30 + 40 + 50);

        // Read every chunk back and confirm the rows survived the builder resets in
        // order, values intact.
        let part_dir = outdir.join("part");
        let mut chunk_paths: Vec<PathBuf> = fs::read_dir(&part_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "parquet"))
            .collect();
        chunk_paths.sort();
        assert_eq!(chunk_paths.len(), 3); // 2 + 2 + 1

        let mut seen: Vec<TestRow> = Vec::new();
        for chunk in chunk_paths {
            let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(&chunk).unwrap())
                .unwrap()
                .build()
                .unwrap();
            for batch in reader {
                let batch = batch.unwrap();
                let paths = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<arrow::array::StringArray>()
                    .unwrap();
                let int_column = |n: usize| {
                    batch
                        .column(n)
                        .as_any()
                        .downcast_ref::<arrow::array::Int64Array>()
                        .unwrap()
                };
                let sizes = int_column(1);
                let uids = int_column(2);
                let gids = int_column(3);
                let modes = int_column(4);
                let atimes = int_column(5);
                let mtimes = int_column(6);
                let ctimes = int_column(7);
                for i in 0..batch.num_rows() {
                    seen.push((
                        paths.value(i).to_string(),
                        sizes.value(i),
                        uids.value(i),
                        gids.value(i),
                        modes.value(i),
                        atimes.value(i),
                        mtimes.value(i),
                        ctimes.value(i),
                    ));
                }
            }
        }
        assert_eq!(seen, rows);
    }

    #[test]
    fn test_finalize_prune_stops_at_first_gap() {
        let dir = tempfile::TempDir::new().unwrap();
        let outdir = dir.path().to_path_buf();
        let mut buf = local_buffer(&outdir, 100);

        // One flush => a single chunk 000000.
        buf.add("/part/a", 10, 1000, 1000, 0o644, 0, 50, 60)
            .unwrap();
        buf.flush().unwrap();

        let part_dir = outdir.join("part");
        // Stale 000001 exists, 000002 is a gap, 000003 exists beyond the gap.
        File::create(part_dir.join(chunk_final_name(1))).unwrap();
        File::create(part_dir.join(chunk_final_name(3))).unwrap();

        let pruned = buf.finalize().unwrap();
        // Prune starts at num_chunks (1): removes 000001, then 000002 is missing -> stop.
        assert_eq!(pruned, 1);
        assert!(part_dir.join(chunk_final_name(0)).exists());
        assert!(!part_dir.join(chunk_final_name(1)).exists());
        assert!(part_dir.join(chunk_final_name(3)).exists()); // untouched beyond the gap
    }
}
