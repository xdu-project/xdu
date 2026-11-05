xdu(1)

# NAME

xdu - build a distributed file metadata index in Parquet format

# SYNOPSIS

*xdu* _DIR_ *-o* _OUTDIR_ \[*-j* _N_] \[*-B* _N_] \[*--apparent-size*] \[*-k* _SIZE_] \[*-p* _NAMES_] \[*--allow-errors*]

# DESCRIPTION

*xdu* crawls a filesystem directory tree and builds a Parquet index containing
file paths, sizes, and access times. The index is partitioned by top-level
subdirectory, enabling per-user queries without full-index scans.

Designed for HPC and enterprise storage environments where traditional tools
like *du*(1) and *find*(1) are too slow for regular auditing of filesystems
with hundreds of millions of files.

The index can be queried with *xdu-find*(1), explored interactively with
*xdu-view*(1), or analyzed directly with tools like DuckDB, Polars, or
Apache Spark.

# OPTIONS

_DIR_
	Top-level directory to index.

*-o*, *--outdir* _DIR_
	Output directory for the Parquet index, or an s3://BUCKET/PREFIX
	locator to build the index into an S3 bucket (see _S3 OUTPUT_). Required.

*-j*, *--jobs* _N_
	Number of parallel threads. Default: 4. See also *XDU_JOBS* in
	_ENVIRONMENT_.

*-B*, *--buffsize* _N_
	Number of records per output Parquet chunk. Default: 100000.

*--apparent-size*
	Report apparent file sizes (file length from stat) rather than actual
	disk usage (st_blocks \* 512). By default, *xdu* reports disk usage.

*-k*, *--block-size* _SIZE_
	Round sizes up to this block size (e.g., 128K, 1M). Useful when
	st_blocks is inaccurate (e.g., over NFS) and you know the filesystem
	block size. Implies *--apparent-size* for the base size, then rounds up.

*-p*, *--partition* _NAMES_
	Index only specific partitions (top-level subdirectory names,
	comma-separated). Useful for incremental re-indexing.

*--allow-errors*
	Continue past unreadable files or directories (permission or I/O
	errors) instead of failing the run. See _EXIT STATUS_.

*-h*, *--help*
	Print help information.

*-V*, *--version*
	Print version information.

# S3 OUTPUT

With *-o* s3://_BUCKET_/_PREFIX_, *xdu* builds the index into the bucket under
that prefix, with the layout and completion marker a local run writes. Each
chunk is sent as one upload at its final key as soon as it is encoded, and
nothing of the index is assembled or staged on local disk. When a partition
finishes, every higher-numbered chunk listed under it is deleted, so a re-run
leaves no stale chunk; unlike the local prune, this one does not stop at a gap
in the numbering.

Before anything is written, *xdu* lists the prefix once. An unreachable
endpoint, rejected credentials or a missing bucket fail the run there, with any
existing index and its marker as they were; an absent or empty prefix starts a
new index. The marker is then deleted, which is the run's first write, so
credentials that may read but not write fail with the marker still in place.
It is written again only after every partition has finished, exactly as for a
local index.

A partition's name becomes part of its keys and of the readers' glob, so
*xdu* refuses, before the marker is touched, a top-level directory whose name
is not valid UTF-8, is a lone dot or two, or holds a character a glob would
expand (\*, ?, [, ], {, }) or the S3 client would percent-encode: anything
outside ASCII, a control character, or one AWS advises against in object keys,
such as ~, %, # or |. Rename the directory to index it.

Credentials, region and endpoint come from the AWS variables and shared files
listed below, the same sources *xdu-find*(1) reads, so the same invocation
reaches AWS S3 or, with an endpoint set, an S3-compatible store. The proxy
variables (HTTP_PROXY, HTTPS_PROXY, NO_PROXY) are not used: requests go
straight to the endpoint. Building an index needs no DuckDB extension; querying
it does (see *xdu-find*(1)).

# ENVIRONMENT

*XDU_JOBS*
	Default number of parallel threads if *-j* is not specified.

*AWS_ACCESS_KEY_ID*, *AWS_SECRET_ACCESS_KEY*
	The key pair for an S3 output. Set together, they win over the shared
	files; one without the other is an error.

*AWS_SESSION_TOKEN*
	A session token sent with that key pair, when set.

*AWS_PROFILE*
	The profile read from the shared files; *default* when unset.

*AWS_SHARED_CREDENTIALS_FILE*
	The shared credentials file, in place of ~/.aws/credentials. The profile's
	key pair is taken from it first, then from the config file.

*AWS_CONFIG_FILE*
	The shared config file, in place of ~/.aws/config. It supplies the
	profile's *region*, and its endpoint: *endpoint_url* under *s3* in the
	profile's *services* section, else the profile's own *endpoint_url*.

*AWS_REGION*, *AWS_DEFAULT_REGION*
	The region, in that order, ahead of the profile's; us-east-1 when none is
	set. With no endpoint set below, the region also names the host,
	s3.REGION.amazonaws.com, so it must be a region name: lowercase letters,
	digits and hyphens.

*AWS_ENDPOINT_URL_S3*, *AWS_ENDPOINT_URL*
	An endpoint to use in place of AWS S3, as http:// or https:// with a host
	and an optional port, in that order ahead of the profile's. Requests to an
	endpoint set this way use path-style addressing.

# EXAMPLES

Index an entire filesystem:

	xdu /data/scratch -o /index/scratch -j 8

Re-index specific partitions:

	xdu /data/scratch -o /index/scratch --partition alice,bob

Index with apparent sizes:

	xdu /data/scratch -o /index/scratch --apparent-size

Index with block size rounding for NFS:

	xdu /data/scratch -o /index/scratch --block-size 128K

Build the index into a bucket, with the key pair of a profile in the shared
files:

	AWS_PROFILE=research xdu /data/scratch -o s3://indexes/scratch

# OUTPUT FORMAT

The index is structured as _OUTDIR_/<partition>/<chunk>.parquet, where
partitions correspond to top-level subdirectories of _DIR_. Files lying
directly in _DIR_ are indexed under the reserved partition name
*\_\_root\_\_*.
Two names in _OUTDIR_ are reserved: that partition, and the completion
marker described below. A real top-level subdirectory of either name would
be written as that entry — clobbering the loose-file partition's chunks, or
occupying the marker path so no run can attest or clear it again — so *xdu*
rejects it with an error rather than corrupting the index; rename the
directory to index it.

Each Parquet file contains eight columns:

- *path* (UTF-8): Absolute file path
- *size* (INT64): File size in bytes
- *uid* (INT64): Owning user id
- *gid* (INT64): Owning group id
- *mode* (INT64): Permission bits (st_mode masked to 07777)
- *atime* (INT64): Last access time as Unix epoch seconds
- *mtime* (INT64): Last modification time as Unix epoch seconds
- *ctime* (INT64): Last inode-change time as Unix epoch seconds

In a local index, files are written atomically using a .partial suffix, then
renamed on completion. Stale chunks from previous runs are automatically
pruned. An S3 index is written differently; see _S3 OUTPUT_.

On success *xdu* writes a run-level completion marker, _OUTDIR_/.xdu-complete,
holding the version and this run's totals. The marker is removed once a run has
passed its pre-flight checks and is about to write, and written only once every
partition has been walked and finalized, so an index left behind by a failed or
interrupted run carries no marker, while a run rejected before it writes leaves
an existing marker intact. Readers glob _OUTDIR_/\*/\*.parquet, so the marker
is never mistaken for a partition, and its name is reserved as above, so a
partition is never written over the marker.

Paths are stored as UTF-8. A filename containing invalid UTF-8 is stored with
replacement characters, so that row no longer names a real file and cannot be
removed with *xdu-rm*(1). *xdu* reports the first such path in each partition
and counts the remainder in its summary; storing the raw bytes would require an
index format change and is not done here.

# EXIT STATUS

*xdu* exits *0* on success. If the crawl cannot read a file or directory
(a permission or I/O error — for example an unreadable subtree that would
otherwise be silently omitted), it reports the offending path and error to
standard error, goes on to index everything else it can reach, and exits
*non-zero* at the end of the run, so a partial index is never presented as
complete. Files that vanish between being listed and being stat'd (an ENOENT
race on a live filesystem) are counted and skipped without failing the run.

A Parquet chunk that cannot be written or renamed stops the run instead. The
partition being written is abandoned — chunks not yet renamed keep
their .partial suffix, which readers ignore — and the other threads finish
the partition they are on but take on no new ones, so partitions still queued
go unindexed.

Either way, a failing run leaves no completion marker (see _OUTPUT FORMAT_).

Pass *--allow-errors* to downgrade the read errors to warnings: the crawl then
indexes every reachable file and exits *0*. Write failures are not downgraded.
The per-path diagnostics and the skipped/error counts in the summary are
printed either way, and all such diagnostics go to standard error so piped
output on standard output stays clean. Such a run still writes the completion
marker, recording the tolerated-error count in it, so *xdu-find*(1),
*xdu-view*(1) and *xdu-rm*(1) warn on standard error that results may be
incomplete. Tolerance can also cost data on a re-index: a partition whose
directory could not be read yields no files, and finalizing it prunes the
chunks that partition previously held, so rows indexed by an earlier run are
removed rather than kept.

# SEE ALSO

*xdu-find*(1), *xdu-view*(1), *xdu-rm*(1)
