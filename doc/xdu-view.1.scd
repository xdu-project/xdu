xdu-view(1)

# NAME

xdu-view - interactive TUI for exploring a file metadata index

# SYNOPSIS

*xdu-view* *-i* _DIR_ \[*-u* _NAME_] \[*-p* _PATTERN_] \[*--regex*] \[*--min-size* _SIZE_]
	\[*--max-size* _SIZE_] \[*--older-than* _DAYS_] \[*--newer-than* _DAYS_]
	\[*-s* _MODE_]

# DESCRIPTION

*xdu-view* provides an ncdu-style terminal user interface for browsing a
Parquet index built by *xdu*(1). It supports two view modes:

- *List mode* (default): Traditional single-list view with directory navigation.
- *Tree mode*: Miller columns showing directory hierarchy left-to-right, with
  file type detection and plain-text preview in the rightmost pane.

All command-line filters can also be modified interactively.

# OPTIONS

*-i*, *--index* _DIR_
	Path to the Parquet index directory, or s3://_BUCKET_/_PREFIX_ for an index
	built into an S3 bucket (see _S3 INDICES_). Required. See also *XDU_INDEX* in
	_ENVIRONMENT_.

*-u*, *--partition* _NAME_
	Start in a specific partition instead of the partition list.

*-p*, *--pattern* _PATTERN_
	Glob pattern to filter file paths. Supports \*, ?, and [...] classes;
	\* crosses directories, so \*.py matches Python files at any depth. A bare
	word matches only that exact name — write \*py\* for a substring search.

*--regex*
	Interpret *-p* as a full regular expression instead of a glob.

*--min-size* _SIZE_
	Minimum file size. Supports suffixes: K, M, G, T.

*--max-size* _SIZE_
	Maximum file size.

*--older-than* _DAYS_
	Show only files not accessed in _DAYS_ days.

*--newer-than* _DAYS_
	Show only files accessed within _DAYS_ days.

*-s*, *--sort* _MODE_
	Sort order. One of: *name* (default, directories first), *size-desc*,
	*size-asc*, *count-desc*, *count-asc*, *age-desc* (oldest first),
	*age-asc* (newest first).

*-h*, *--help*
	Print help information.

*-V*, *--version*
	Print version information.

# KEYBINDINGS

## Navigation

*↑*/*k*
	Move selection up.

*↓*/*j*
	Move selection down.

*→*/*Enter*
	Enter directory.

*Space* (list mode)
	Open a preview overlay for the selected file. No-op on a
	directory. *Esc* dismisses. *Enter* and *→* still enter
	directories.

*Space* (tree mode)
	Enter directory, or focus the preview pane when a file is
	selected.

*←*/*Backspace*
	Go up / back.

*g*
	Jump to first entry.

*G*
	Jump to last entry.

*t*
	Toggle between list and tree view modes.

*q*/*Esc*
	Quit.

## Sorting

*s*
	Open sort selector. Use *↑↓*/*jk* to choose, *Enter* to confirm,
	*Esc* to cancel.

## Filtering (interactive)

*/*
	Set path pattern filter (glob).

*o*
	Set older-than filter (days).

*n*
	Set newer-than filter (days).

*>*
	Set minimum size filter.

*<*
	Set maximum size filter.

*c*
	Clear all filters.

## Tree View Preview

*Tab*
	Toggle focus to the file preview pane.

*j*/*k* (when preview focused)
	Scroll preview content.

*Esc* (when preview focused)
	Return focus to the directory columns.

# S3 INDICES

An index under an S3 prefix is browsed in place. Each view reads the Parquet
footers and only the parts of each chunk it needs, through HTTP range requests,
so listing a directory costs a fraction of the index's size. No part of the
index is copied to local disk, and nothing is downloaded at run time. The tree
view's file preview reads the file itself, not the index, so it shows a file
only where its path exists on this host.

Reading an S3 index needs the DuckDB *httpfs* extension, which an xdu release
installs beside the binaries; see *XDU_EXTENSION_DIR* below. A local index loads
no extension.

Credentials, region and endpoint come from the AWS variables and shared files
listed below, so the same invocation reaches AWS S3 or, with an endpoint set,
an S3-compatible store. The proxy variables (HTTP_PROXY, HTTPS_PROXY, NO_PROXY)
are not used: requests go straight to the endpoint.

The bucket must have a name S3 allows: 3 to 63 lowercase letters, digits, dots
and hyphens, beginning and ending with a letter or digit, with no two dots
together. A locator naming any other bucket is refused before a request is
sent, and so is one carrying credentials before the bucket; give those through
the variables below. No such refusal repeats the locator.

# ENVIRONMENT

*XDU_INDEX*
	Default index if *-i* is not specified: a directory or an s3:// locator.

*XDU_EXTENSION_DIR*
	Directory holding the DuckDB extensions an S3 index needs, in place of
	lib/xdu/extensions under the installation prefix. A *cargo install* build
	ships none: copy lib/xdu/extensions/ from the xdu release tarball for this
	version and platform into the prefix, or set this variable to a directory
	holding them. An unset or empty value means the default.

*AWS_ACCESS_KEY_ID*, *AWS_SECRET_ACCESS_KEY*
	The key pair for an S3 index. Set together, they win over the shared files;
	one without the other is an error.

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

Browse all partitions:

	xdu-view -i /index/scratch

Start in a specific partition:

	xdu-view -i /index/scratch -u alice

View only stale files, sorted by size:

	xdu-view -i /index/scratch --older-than 30 -s size-desc

View large Python files:

	xdu-view -i /index/scratch -p '\*.py' --min-size 1M

Browse an index built into a bucket, with the key pair of a profile in the
shared files:

	AWS_PROFILE=research xdu-view -i s3://indexes/scratch

# SEE ALSO

*xdu*(1), *xdu-find*(1), *xdu-rm*(1)
