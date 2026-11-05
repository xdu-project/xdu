xdu-rm(1)

# NAME

xdu-rm - delete files matching index query criteria

# SYNOPSIS

*xdu-rm* *-i* _DIR_ \[*-u* _NAME_] \[*-p* _PATTERN_] \[*--regex*] \[*--min-size* _SIZE_]
	\[*--max-size* _SIZE_] \[*--older-than* _DAYS_] \[*--newer-than* _DAYS_]
	\[*-l* _N_] \[*-n*] \[*--safe*] \[*-f*] \[*-v*] \[*-j* _N_]

# DESCRIPTION

*xdu-rm* deletes files that match query criteria against a Parquet index
built by *xdu*(1). It shares the path, size, and access-time filter options
with *xdu-find*(1) — but not the owner, group, permission, or modification-time
filters — to select files for deletion and supports parallel processing for
efficient bulk removal.

Before deleting, *xdu-rm* displays a summary of matched files and prompts for
confirmation unless *--force* is given. Use *--dry-run* to preview what would
be deleted without removing anything.

# OPTIONS

*-i*, *--index* _DIR_
	Path to the Parquet index directory. Required. See also *XDU_INDEX* in
	_ENVIRONMENT_. An index in S3, given as s3://BUCKET/PREFIX, is refused:
	*xdu-rm* does not yet delete through an S3 index.

*-u*, *--partition* _NAME_
	Filter by partition (top-level subdirectory name).

*-p*, *--pattern* _PATTERN_
	Glob pattern to match file paths. Supports \*, ?, and [...] classes;
	\* crosses directories, so \*.tmp matches temporary files at any depth.
	A bare word matches only that exact name — write \*tmp\* for a substring
	search.

*--regex*
	Interpret *-p* as a full regular expression instead of a glob.

*--min-size* _SIZE_
	Minimum file size. Supports suffixes: K, M, G, T.

*--max-size* _SIZE_
	Maximum file size.

*--older-than* _DAYS_
	Delete only files not accessed in _DAYS_ days.

*--newer-than* _DAYS_
	Delete only files accessed within _DAYS_ days.

*-l*, *--limit* _N_
	Delete at most _N_ files, selecting the _N_ whose paths sort first in
	ascending lexicographic order. The selection is deterministic, so a
	*--dry-run* preview lists exactly the files a subsequent real run deletes.

*-n*, *--dry-run*
	Show what would be deleted without actually deleting anything.

*--safe*
	Verify file metadata before deletion by re-stating each file to confirm
	its access time and size still match the index. Protects against deleting
	files that have been modified since the index was built.

*-f*, *--force*
	Skip the interactive confirmation prompt.

*-v*, *--verbose*
	Show detailed output for each file as it is deleted.

*-j*, *--jobs* _N_
	Number of parallel threads for deletion. Default: 4. See also *XDU_JOBS*
	in _ENVIRONMENT_.

*-h*, *--help*
	Print help information.

*-V*, *--version*
	Print version information.

# ENVIRONMENT

*XDU_INDEX*
	Default index directory if *-i* is not specified.

*XDU_JOBS*
	Default number of parallel threads if *-j* is not specified.

# EXAMPLES

Preview deletion of stale files:

	xdu-rm -i /index/scratch -u alice --older-than 180 -n

Delete temporary files, skipping confirmation:

	xdu-rm -i /index/scratch -p '\*.tmp' --force

Safe deletion of large files (re-check metadata before removing):

	xdu-rm -i /index/scratch --min-size 1G --safe

Delete with verbose output and 8 threads:

	xdu-rm -i /index/scratch --older-than 90 -v -j 8

# SEE ALSO

*xdu*(1), *xdu-find*(1), *xdu-view*(1)
