xdu-cp(1)

# NAME

xdu-cp - copy files matching index query criteria into a directory tree

# SYNOPSIS

*xdu-cp* *-i* _DIR_ *--into* _DIR_ \[*--root* _DIR_] \[*-u* _NAME_] \[*-p* _PATTERN_]
	\[*--regex*] \[*--min-size* _SIZE_] \[*--max-size* _SIZE_]
	\[*--older-than* _DAYS_] \[*--newer-than* _DAYS_] \[*--owner* _USER_]
	\[*--group* _GROUP_] \[*--mode* _SPEC_] \[*--mtime-older-than* _DAYS_]
	\[*--mtime-newer-than* _DAYS_] \[*-l* _N_] \[*-n*] \[*-f*] \[*-v*] \[*-j* _N_]

# DESCRIPTION

*xdu-cp* copies the files that match query criteria against a Parquet index
built by *xdu*(1) into a tree under *--into*. It accepts every selection filter
*xdu-find*(1) accepts, with the same meaning, and the action flags of
*xdu-rm*(1).

Each selected file is copied to _INTO_/_REL_, where _REL_ is its indexed path
relative to *--root*. Missing directories are created on the way. A file is
written under a temporary name in its final directory, flushed to stable
storage, and then given its final name by a rename that refuses to replace
anything, so a destination name only ever holds a complete copy. An existing
file at a destination is never replaced or truncated: it is skipped, counted,
and makes the run exit non-zero.

*xdu-cp* never modifies or removes a source file. It copies only a regular
file at the indexed path and follows no symlink, neither at the path nor in
any directory above it, nor anywhere under *--into*. A symlink, directory,
FIFO, socket or device found at a selected path is skipped and counted, and is
never opened.

Before copying, *xdu-cp* asks for confirmation unless *--force* is given. Use
*--dry-run* to list each source with its destination without writing anything.

# OPTIONS

*-i*, *--index* _DIR_
	Path to the Parquet index directory. Required. See also *XDU_INDEX* in
	_ENVIRONMENT_. An index in S3, given as s3://BUCKET/PREFIX, is refused
	before anything else runs.

*--into* _DIR_
	Existing directory to copy into. Required. It must exist and be a
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
	\* crosses directories, so \*.log matches log files at any depth.

*--regex*
	Interpret *-p* as a full regular expression instead of a glob.

*--min-size* _SIZE_
	Minimum file size. Supports suffixes: K, M, G, T.

*--max-size* _SIZE_
	Maximum file size.

*--older-than* _DAYS_
	Copy only files not accessed in _DAYS_ days.

*--newer-than* _DAYS_
	Copy only files accessed within _DAYS_ days.

*--owner* _USER_
	Copy only files owned by _USER_, a name or a numeric uid.

*--group* _GROUP_
	Copy only files owned by _GROUP_, a name or a numeric gid.

*--mode* _SPEC_
	Permission filter, as in *xdu-find*(1): bare octal matches exactly
	(644), /OCTAL matches when any listed bit is set (/002), &OCTAL when all
	are (&4000).

*--mtime-older-than* _DAYS_
	Copy only files not modified in _DAYS_ days.

*--mtime-newer-than* _DAYS_
	Copy only files modified within _DAYS_ days.

*-l*, *--limit* _N_
	Copy at most _N_ files, the _N_ whose paths sort first in ascending
	lexicographic order. The selection is deterministic, so a *--dry-run*
	preview lists exactly the files a subsequent real run copies.

*-n*, *--dry-run*
	Print each source and its destination, one "SOURCE -> DESTINATION" line
	per file in path order, and the file count. Nothing is written: no
	directory, no file.

*--safe*
	Not available yet: passing it refuses the run, and nothing is copied.

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

# DESTINATION LAYOUT

With an index of the tree /scratch, *--root /scratch* and *--into /cold*, the
indexed file /scratch/alice/logs/app.log is copied to

	/cold/alice/logs/app.log

and with the default *--root* of / the same file is copied to
/cold/scratch/alice/logs/app.log instead.

# REFUSALS

The run stops with a message on standard error, having written nothing, when:

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
copied, dry-run included, and name the first offending path in path order.

# OUTPUT

Standard output carries the *--dry-run* listing, the *-v* listing, the message
for an empty match, and the summary: the number of files copied, then each
other outcome that occurred (missing, existing at destination, skipped,
failed), each counted separately. The confirmation prompt, warnings and every
failure, with its path and cause, go to standard error.

# EXIT STATUS

*0*
	Every selected file was copied, missing or skipped; or nothing matched;
	or the prompt was declined.

*1*
	A file failed or already existed at its destination, or the run was
	refused before anything was written.

# FILES

A copy in progress lives in its destination directory under a name of the
form .xdu-PID-SEQ-RANDOM.part. A failed copy removes it; an interrupted
run (a crash, a power loss, a kill) can leave one behind, never a partial
file at a final name. Find leftovers with

	find INTO -name '.xdu-\*.part'

# ENVIRONMENT

*XDU_INDEX*
	Default index directory if *-i* is not specified.

*XDU_JOBS*
	Default number of parallel threads if *-j* is not specified.

# EXAMPLES

Preview copying a user's year-old files into a staging tree:

	xdu-cp -i /index/scratch -u alice --older-than 365 --into /stage --root /scratch -n

Copy every log file, skipping confirmation:

	xdu-cp -i /index/scratch -p '\*.log' --into /stage --root /scratch --force

Copy large files, mirroring their absolute paths, listing each one:

	xdu-cp -i /index/scratch --min-size 1G --into /stage -v

# SEE ALSO

*xdu*(1), *xdu-find*(1), *xdu-rm*(1), *xdu-view*(1)
