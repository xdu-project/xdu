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

A file whose destination is on another filesystem cannot be renamed there.
It is copied instead, flushed to stable storage, checked against its source,
and only then removed from its source; see _CROSSING FILESYSTEMS_.

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
and the index does not record which. So when *--min-size* or *--max-size* is
given, two rules apply, and a file that fails either is skipped. First, the
size filter must hold for both live measures, the length and the disk usage.
Second, the file's modification time must still equal the one the index
recorded, to the second, or the file is skipped as changed since the index
was built. The second rule covers what the first cannot see: on an index built
with *--block-size 1M*, a file indexed at 900 KiB (one block) and since grown
to 1.2 MB (two blocks) passes *--max-size 1500K* by both live measures.

Both rules fail closed. A file selected by *--min-size 2K* that has shrunk to
100 bytes is skipped whatever its block count. They also skip some unchanged
files: a 100-byte file in a 4 KiB block holds *--max-size 1K* by its length
but not by its disk usage, so give *--max-size* a bound no smaller than the
filesystem's block size when you combine it with *--safe*; and a file whose
modification time was set after the crawl, by *touch*(1) say, is skipped
though its bytes did not change. A change made within the same second as the
modification time the index recorded leaves that time equal, and only the
first rule then applies.

The check and the rename are two steps. An object put in place of the file
at the same name between them is renamed as itself: a rename never follows a
symlink, so nothing outside the selected path is reached, but whatever holds
the name at that instant is what moves. A file that has to be copied to
another filesystem is checked again against the metadata of the very file
opened for the copy.

# CROSSING FILESYSTEMS

A rename cannot cross filesystems. When the kernel answers a rename by
saying the destination is on another filesystem, *xdu-mv* moves that file by
copying it there and then removing its source. The rename's answer decides,
not a comparison of device numbers, which agree across a bind mount and can
differ within one filesystem. For each such file, *xdu-mv*:

- opens the source, refusing anything but a regular file, and checks that
  this user may remove names from its directory, failing the file before
  anything is copied if not;
- copies its bytes to a temporary file in the destination directory, under
  a name of the form .xdu-PID-SEQ-RANDOM.part, and gives the copy the
  source's owner and group where permitted, its permission bits, and its
  access and modification times;
- flushes the copy to stable storage, checks that the source's size,
  modification time and change time did not move while it was read, and
  gives the copy its final name with a rename that refuses to replace
  anything;
- flushes the destination directory;
- checks that the source name still holds the file it opened, with the same
  size, modification time and change time;
- removes the source name, and flushes the source directory.

The source is removed only after both flushes have returned, so an
interruption at any point (a crash, a power loss, a kill) leaves each file
whole at its source, at its destination, or at both, never at neither and
never partial at a final name. Each move to another filesystem waits for two
flushes, a few milliseconds each on a local disk, so many small files move
markedly slower across filesystems than within one.

A source written while it was copied, or changed or replaced before its
removal, is skipped and counted, its copy is removed, and it stays at its
source. A source that cannot be removed (a read-only filesystem, or a sticky
directory and a file this user does not own) is counted as failed with the
cause on standard error; its copy is removed and it stays at its source,
since a copy left beside its source would read as an existing destination
on a re-run. A source that disappears after it was copied and before it was
removed is counted as failed and its copy is kept, the only whole instance
left. A rename carries ownership with the inode; a copy carries it only where
permitted, and a refusal is counted as *Ownership not preserved*, as in
*xdu-cp*(1), not as a failure.

The checks narrow some races but cannot close them. A write between the last
check and the removal is lost with the source. A process holding the source
open for writing keeps writing to the removed file. A writer through a shared
memory map may not move the modification time, and a same-size rewrite
within one tick of the filesystem clock moves neither time. A different file
renamed onto the source name, inside its directory, between the last check
and the removal is removed in its place. Do not move files that are still
being written.

On a filesystem without a rename that refuses to replace (NFS and some FUSE
filesystems), *xdu-mv* links the file at its destination, flushes the
destination directory, checks that the source name still holds the same
file, and only then removes the source name; a copy to another filesystem
publishes its temporary file the same way. An interruption between the link
and the removal leaves the file under both names, one inode. A destination
that supports neither that rename nor hard links fails each file with a
message.

# INDEX AND ACCESS TIMES

The index is not updated. A moved file keeps its row under its old path, and
its new path has none, until the trees are indexed again; a later run of the
same selection from the same index counts the moved files as missing.

Reading a source may update its atime under the mount's options (*relatime*,
*strictatime* or *noatime*), which can move a file out of a later
*--older-than* selection made from a new index, or fail a later *--safe*
re-check of *--older-than* against an older index. A move within one
filesystem is a rename and reads no file, so it leaves each file's access time
as it was. A move to another filesystem reads the source, and gives the copy
the access time the source had before that read.

# DESTINATION LAYOUT

With an index of the tree /projects, *--root /projects* and *--into /archive*,
the indexed file /projects/carol/runs/r1.h5 is moved to

	/archive/carol/runs/r1.h5

and with the default *--root* of / the same file is moved to
/archive/projects/carol/runs/r1.h5 instead.

# PERMISSIONS

*xdu-mv* reaches each source and each destination one directory at a time,
and needs only search (execute) permission on a directory it passes through:
a directory of mode 0711 is no obstacle to a user other than its owner. The
directory holding a source needs write and search permission, to remove the
name, and read permission as well: after a move to another filesystem
*xdu-mv* flushes that directory to stable storage, and a flush needs read
permission, so it opens every source directory for reading. Each directory a
file lands in is opened for reading for the same reason. Where such an open
is refused, every file in or bound for that directory fails with the
directory named on standard error and stays at its source, and the run exits
non-zero.

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
- *Skipped*: not a regular file, no longer matching a filter under
  *--safe*, or changed while it was copied to another filesystem; the
  source stays;
- *Failed*: an error, reported on standard error with the path and cause;
  the source stays.

- *Ownership not preserved*: moved to another filesystem, where the copy
  could not be given the source's owner or group; not a failure.

A rename carries the file's owner and group with its inode, so only a move
to another filesystem can be counted as *Ownership not preserved*.

The *-v* listing prints one line per file in path order: "moved: SOURCE ->
DESTINATION", "moved (ownership not preserved): SOURCE -> DESTINATION",
"existing: DESTINATION", "missing: SOURCE" or "skipped (REASON): SOURCE".
The confirmation prompt, warnings and every failure go to standard error.

# EXIT STATUS

*0*
	Every selected file was moved, missing or skipped; or nothing matched;
	or the prompt was declined.

*1*
	A file failed or already existed at its destination, or the run was
	refused before anything was written. A re-run after an interrupted
	move therefore reports the files already moved as missing, and exits
	non-zero for any whose destination is taken.

# FILES

A copy to another filesystem lives in its destination directory under a name
of the form .xdu-PID-SEQ-RANDOM.part until it is complete. A failed copy
removes it; an interrupted run can leave one behind, never a partial file at
a final name. Find leftovers with

	find INTO -name '.xdu-\*.part'

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
