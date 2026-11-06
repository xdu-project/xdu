xdu-mv(1)

# NAME

xdu-mv - move files matching index query criteria into a directory tree

# SYNOPSIS

*xdu-mv* *-i* _DIR_ *--into* _DIR_ \[*--root* _DIR_] \[*-u* _NAME_] \[*-p* _PATTERN_]
	\[*--regex*] \[*--min-size* _SIZE_] \[*--max-size* _SIZE_]
	\[*--older-than* _DAYS_] \[*--newer-than* _DAYS_] \[*--owner* _USER_]
	\[*--group* _GROUP_] \[*--mode* _SPEC_] \[*--mtime-older-than* _DAYS_]
	\[*--mtime-newer-than* _DAYS_] \[*-l* _N_] \[*-n*] \[*--safe*] \[*-f*] \[*-v*]
	\[*-j* _N_]

# DESCRIPTION

*xdu-mv* moves the files that match query criteria against a Parquet index
built by *xdu*(1) into a tree under *--into*. It accepts every selection filter
*xdu-find*(1) accepts, with the same meaning, and the action flags of
*xdu-rm*(1). It shares its selection, refusals, preview, confirmation and
summary with *xdu-cp*(1).

Each selected file is moved to _INTO_/_REL_, where _REL_ is its indexed path
relative to *--root*. Missing directories are created on the way. A move is a
rename that refuses to replace anything, so the file keeps its inode, and with
it its bytes, owner, group, permission bits and times. An existing file at a
destination is never replaced or truncated: it is skipped, counted, left
untouched, and makes the run exit non-zero; the source stays where it was.

This build moves files within one filesystem only; see _CROSSING FILESYSTEMS_.

*xdu-mv* moves only a regular file at the indexed path, on every run, with or
without *--safe*. It follows no symlink, neither at the path nor in any
directory above it, nor anywhere under *--into*. A symlink, directory, FIFO,
socket or device found at a selected path is skipped, counted and left where
it is: a directory there would otherwise carry its whole subtree with it.

Before moving, *xdu-mv* asks for confirmation unless *--force* is given. Use
*--dry-run* to list each source with its destination without writing anything.

# OPTIONS

*-i*, *--index* _DIR_
	Path to the Parquet index directory. Required. See also *XDU_INDEX* in
	_ENVIRONMENT_. An index in S3, given as s3://BUCKET/PREFIX, is refused
	before anything else runs.

*--into* _DIR_
	Existing directory to move into. Required. It must exist and be a
	directory; a symlink naming one is followed once.

*--root* _DIR_
	Prefix stripped from each indexed path to form its place under *--into*.
	Default: /, which reproduces each absolute path under *--into*. Index rows
	hold the canonical path of the crawled tree, so *--root* is canonicalized
	before it is compared.

*-u*, *--partition* _NAME_
	Filter by partition (top-level subdirectory name). The partition must
	exist in the index.

*-p*, *--pattern* _PATTERN_
	Glob pattern to match file paths. Supports \*, ?, and [...] classes;
	\* crosses directories, so \*.h5 matches HDF5 files at any depth.

*--regex*
	Interpret *-p* as a full regular expression instead of a glob.

*--min-size* _SIZE_
	Minimum file size. Supports suffixes: K, M, G, T.

*--max-size* _SIZE_
	Maximum file size.

*--older-than* _DAYS_
	Move only files not accessed in _DAYS_ days.

*--newer-than* _DAYS_
	Move only files accessed within _DAYS_ days.

*--owner* _USER_
	Move only files owned by _USER_, a name or a numeric uid.

*--group* _GROUP_
	Move only files owned by _GROUP_, a name or a numeric gid.

*--mode* _SPEC_
	Permission filter, as in *xdu-find*(1): bare octal matches exactly
	(644), /OCTAL matches when any listed bit is set (/002), &OCTAL when all
	are (&4000).

*--mtime-older-than* _DAYS_
	Move only files not modified in _DAYS_ days.

*--mtime-newer-than* _DAYS_
	Move only files modified within _DAYS_ days.

*-l*, *--limit* _N_
	Move at most _N_ files, the _N_ whose paths sort first in ascending
	lexicographic order. The selection is deterministic, so a *--dry-run*
	preview lists exactly the files a subsequent real run moves.

*-n*, *--dry-run*
	Print each source and its destination, one "SOURCE -> DESTINATION" line
	per file in path order, and the file count. Nothing is written: no
	directory is created, and no file is moved.

*--safe*
	Re-check each file against its live metadata immediately before moving
	it, and skip it, counted, unless it is still a regular file and every
	filter given still holds. See _SAFE MODE_.

*-f*, *--force*
	Skip the interactive confirmation prompt. Here *-f* is *--force*, as in
	*xdu-rm*(1), not the *--format* of *xdu-find*(1).

*-v*, *--verbose*
	List each file and its outcome on standard output.

*-j*, *--jobs* _N_
	Number of parallel threads. Default: 4. See also *XDU_JOBS* in
	_ENVIRONMENT_.

*-h*, *--help*
	Print help information.

*-V*, *--version*
	Print version information.

# SAFE MODE

The index is a snapshot, so a file can change between the crawl and the move.
With *--safe*, *xdu-mv* reads the metadata of the object at each selected path
just before renaming it, without following a symlink, and skips the file
unless it is still a regular file and every filter given still holds against
that live metadata: *--min-size* and *--max-size* against its size,
*--older-than* and *--newer-than* against its access time,
*--mtime-older-than* and *--mtime-newer-than* against its modification time,
*--owner* against its uid, *--group* against its gid, and *--mode* against its
permission bits. The time thresholds are the ones the query used, fixed when
the run started. The path pattern and the partition hold by construction. A
skipped file is listed under *-v* with the filter it no longer matches,
counted, and left at its source.

Size needs a measure. The indexed size is the disk usage of the file (its
blocks times 512) by default, its length when the index was built with
*--apparent-size*, or its length rounded up to a block with *--block-size*,
and the index does not record which. So a size filter must hold for both
live measures, the length and the disk usage, or the file is skipped. That
fails closed: a file selected by *--min-size 2K* that has shrunk to 100 bytes
is skipped whatever its block count. It also skips some unchanged files: a
100-byte file in a 4 KiB block holds *--max-size 1K* by its length but not by
its disk usage, so give *--max-size* a bound no smaller than the filesystem's
block size when you combine it with *--safe*.

The check and the rename are two steps. An object put in place of the file
at the same name between them is renamed as itself: a rename never follows a
symlink, so nothing outside the selected path is reached, but whatever holds
the name at that instant is what moves.

# CROSSING FILESYSTEMS

A rename cannot cross filesystems. This build does not copy a file to
another filesystem: a file whose destination is on a different filesystem
from its source is not moved, is reported on standard error as "crossing
filesystems is not supported by this build", is counted as failed, and stays
at its source untouched. Directories already created for it under *--into*
remain. Use *xdu-cp*(1) to copy such files.

On a filesystem without a rename that refuses to replace (NFS and some FUSE
filesystems), *xdu-mv* links the file at its destination, flushes the
destination directory, checks that the source name still holds the same
file, and only then removes the source name. An interruption between the
link and the removal leaves the file under both names, one inode. A
destination that supports neither that rename nor hard links fails each file
with a message.

# INDEX AND ACCESS TIMES

The index is not updated. A moved file keeps its row under its old path, and
its new path has none, until the trees are indexed again; a later run of the
same selection from the same index counts the moved files as missing.

Reading a source may update its atime under the mount's options (*relatime*,
*strictatime* or *noatime*), which can move a file out of a later
*--older-than* selection made from a new index, or fail a later *--safe*
re-check of *--older-than* against an older index. A move within one
filesystem is a rename and reads no file, so it leaves each file's access time
as it was.

# DESTINATION LAYOUT

With an index of the tree /projects, *--root /projects* and *--into /archive*,
the indexed file /projects/carol/runs/r1.h5 is moved to

	/archive/carol/runs/r1.h5

and with the default *--root* of / the same file is moved to
/archive/projects/carol/runs/r1.h5 instead.

# REFUSALS

The run stops with a message on standard error, having moved nothing and
written nothing, when:

- the index is in S3;
- *--into* does not exist or is not a directory;
- *--root* does not exist;
- the index path contains a glob character (\*, ?, [ or \\), which could
  make the reader select another index; or a partition name is empty, is .
  or .., contains /, contains a glob character, or names no partition in the
  index;
- any selected path is not under *--root*, is not an absolute, normalized
  path, or maps to a destination equal to its own source path.

The last checks run over the whole selected set before the first file is
moved, dry-run included, and name the first offending path in path order.

# OUTPUT

Standard output carries the *--dry-run* listing, the *-v* listing, the message
for an empty match, and the summary: the number of files moved, then each
other outcome that occurred, each counted separately:

- *Missing*: nothing at the indexed path, or a directory above it is gone;
- *Existing at destination*: something already held the destination name,
  and both it and the source were left untouched;
- *Skipped*: not a regular file, or no longer matching a filter under
  *--safe*; the source stays;
- *Failed*: an error, reported on standard error with the path and cause;
  the source stays.

A move carries the file's owner and group with its inode, so the
*Ownership not preserved* count of *xdu-cp*(1) does not occur here.

The *-v* listing prints one line per file in path order: "moved: SOURCE ->
DESTINATION", "existing: DESTINATION", "missing: SOURCE" or "skipped
(REASON): SOURCE". The confirmation prompt, warnings and every failure go to
standard error.

# EXIT STATUS

*0*
	Every selected file was moved, missing or skipped; or nothing matched;
	or the prompt was declined.

*1*
	A file failed or already existed at its destination, or the run was
	refused before anything was written. A re-run after an interrupted
	move therefore reports the files already moved as missing, and exits
	non-zero for any whose destination is taken.

# ENVIRONMENT

*XDU_INDEX*
	Default index directory if *-i* is not specified.

*XDU_JOBS*
	Default number of parallel threads if *-j* is not specified.

# EXAMPLES

Preview moving a user's files unmodified for two years into an archive tree:

	xdu-mv -i /index/projects -u carol --mtime-older-than 730 --into /archive --root /projects -n

Move every HDF5 file, re-checking each against its live metadata first and
skipping confirmation:

	xdu-mv -i /index/projects -p '\*.h5' --into /archive --root /projects --safe --force

Move year-old files, mirroring their absolute paths, listing each one:

	xdu-mv -i /index/projects --older-than 365 --into /archive -v

# SEE ALSO

*xdu*(1), *xdu-cp*(1), *xdu-find*(1), *xdu-rm*(1), *xdu-view*(1)
