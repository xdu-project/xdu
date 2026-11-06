xdu-find(1)

# NAME

xdu-find - query a file metadata index for matching paths

# SYNOPSIS

*xdu-find* *-i* _DIR_ \[*-p* _PATTERN_] \[*--regex*] \[*-u* _NAME_] \[*--min-size* _SIZE_] \[*--max-size* _SIZE_]
	\[*--older-than* _DAYS_] \[*--newer-than* _DAYS_] \[*--owner* _USER_] \[*--group* _GROUP_]
	\[*--mode* _SPEC_] \[*--mtime-older-than* _DAYS_] \[*--mtime-newer-than* _DAYS_]
	\[*-f* _FORMAT_] \[*-l* _N_]
	\[*-c*] \[*--top* _N_]

# DESCRIPTION

*xdu-find* queries a Parquet index built by *xdu*(1) for files matching the
given filter criteria. Results are printed to standard output in the requested
format, suitable for piping to other tools.

Uses DuckDB for efficient Parquet queries with partition pruning.

# OPTIONS

*-i*, *--index* _DIR_
	Path to the Parquet index directory, or s3://_BUCKET_/_PREFIX_ for an index
	built into an S3 bucket (see _S3 INDICES_). Required. See also *XDU_INDEX* in
	_ENVIRONMENT_.

*-p*, *--pattern* _PATTERN_
	Glob pattern to match file paths. Supports \*, ?, and [...] classes;
	\* crosses directories, so \*.py matches Python files at any depth. A bare
	word matches only that exact name — write \*py\* for a substring search.

*--regex*
	Interpret *-p* as a full regular expression instead of a glob.

*-u*, *--partition* _NAME_
	Filter by partition (top-level subdirectory name, typically a username).

*--min-size* _SIZE_
	Minimum file size. Supports suffixes: K, M, G, T (and KiB, MiB, GiB,
	TiB variants).

*--max-size* _SIZE_
	Maximum file size. Same suffix support as *--min-size*.

*--older-than* _DAYS_
	Match files not accessed in _DAYS_ days.

*--newer-than* _DAYS_
	Match files accessed within _DAYS_ days.

*--owner* _USER_
	Match files owned by _USER_: a user name resolved at query time, or a
	numeric uid. An unresolvable name fails without printing rows.

*--group* _GROUP_
	Match files owned by group _GROUP_: a group name resolved at query time,
	or a numeric gid. An unresolvable name fails without printing rows.

*--mode* _SPEC_
	Match permission bits. Bare octal matches exactly (*644*); a */* prefix
	matches when any listed bit is set (*/002* is world-writable); an *&*
	prefix matches when all listed bits are set (*&4000* is setuid).

*--mtime-older-than* _DAYS_
	Match files not modified in _DAYS_ days.

*--mtime-newer-than* _DAYS_
	Match files modified within _DAYS_ days.

*-f*, *--format* _FORMAT_
	Output format. One of: *path* (default), *size*, *atime*, *csv*, *json*.

*-l*, *--limit* _N_
	Limit number of results.

*-c*, *--count*
	Print the count of matching records instead of listing them.

*--top* _N_
	Show the top _N_ partitions by file count.

*-h*, *--help*
	Print help information.

*-V*, *--version*
	Print version information.

# S3 INDICES

An index under an S3 prefix is queried in place. Each query reads the Parquet
footers and only the parts of each chunk it needs, through HTTP range requests,
so a *--count* costs a small fraction of the index's size. No part of the index
is copied to local disk, and nothing is downloaded at run time.

Reading an S3 index needs the DuckDB *httpfs* extension, which an xdu release
installs beside the binaries; see *XDU_EXTENSION_DIR* below. A local index loads
no extension.

Credentials, region and endpoint come from the AWS variables and shared files
listed below, so the same invocation reaches AWS S3 or, with an endpoint set,
an S3-compatible store. The proxy variables (HTTP_PROXY, HTTPS_PROXY, NO_PROXY)
are not used: requests go straight to the endpoint. *xdu-rm*(1) refuses S3
indices.

Every connection to an https endpoint, which is every AWS one, authenticates
the server's certificate against one CA file: the file *AWS_CA_BUNDLE* names,
else the first of /etc/ssl/certs/ca-certificates.crt,
/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem,
/etc/pki/tls/certs/ca-bundle.crt, /etc/ssl/ca-bundle.pem and /etc/ssl/cert.pem
that exists. SSL_CERT_FILE, SSL_CERT_DIR and the macOS keychain are not read,
and nothing turns verification off. A certificate that does not chain to a CA
in that file is refused, with an error naming the certificate, before any row
is read; a missing or unreadable file, or one holding no CA certificate, is
refused before any request. A plain http:// endpoint does not use TLS, and
none of this applies to it.

A private CA's certificate for the endpoint must name the endpoint's host in
its subject alternative name (SAN), and the CA file must include the root
certificate its chain ends at: a file holding no self-signed root, such as an
intermediate CA alone, is refused before any request. That root must be
self-signed with SHA-256 or stronger, since a file whose only roots are
SHA-1-signed is refused the same way, and an intermediate must ship with its
own root: beside an unrelated root, *xdu*(1) writes an index that xdu-find
refuses. Known limitation: xdu-find and *xdu-view*(1) accept a certificate whose
subject common name (CN) names the host when its SAN does not, or when it has
none, while *xdu*(1) refuses it. A follow-up will make the readers check the
SAN alone.

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

*AWS_CA_BUNDLE*
	A PEM file of the CA certificates an https endpoint's certificate must
	chain to, in place of the platform's bundle. An empty value means unset.
	Not read for an http:// endpoint.

# EXAMPLES

Find all Python files:

	xdu-find -i /index/scratch -p '\*.py'

Find large files not accessed in 90 days:

	xdu-find -i /index/scratch --min-size 1G --older-than 90

Query a specific user's partition, sorted by size:

	xdu-find -i /index/scratch -u alice --min-size 100M -f size

Count matching files:

	xdu-find -i /index/scratch -p '\*.tmp' --older-than 30 --count

Pipe results to xargs for bulk operations:

	xdu-find -i /index/scratch -p '\*.tmp' --older-than 30 | xargs rm

The same search with a regular expression:

	xdu-find -i /index/scratch --regex -p '\\.tmp$' --older-than 30 --count

Show top 10 partitions by file count:

	xdu-find -i /index/scratch --top 10

Count the files in an index built into a bucket, with the key pair of a
profile in the shared files:

	AWS_PROFILE=research xdu-find -i s3://indexes/scratch --count

# SEE ALSO

*xdu*(1), *xdu-view*(1), *xdu-rm*(1)
