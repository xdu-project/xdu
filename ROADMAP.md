# XDU Roadmap

xdu is a high-performance filesystem indexer and query suite for HPC and enterprise storage, where
`du` and `find` collapse under billion-file trees. It builds a persistent Parquet index once,
partitioned by top-level subdirectory, then answers size/age/pattern questions instantly via
DuckDB — on the command line (`xdu-find`), in an interactive TUI (`xdu-view`), or through guarded
bulk deletion (`xdu-rm`). This document records the larger-scale features still intended: reaching
beyond the local disk to object storage and change-stream ingestion, beyond `path/size/atime` to
richer metadata (delivered on `main`), and beyond the terminal to the browser.

This is a **forward-looking roadmap, not an implementation plan.** Each entry states a user problem
and the intention behind solving it — a seed for `/xdu-feature` to shape into a `GOAL.md`, leaving
the *how* to `/xdu-plan`. As-built architecture and the load-bearing invariants live in
[`AGENTS.md`](AGENTS.md); this file is only about what comes next. Horizons below (near / mid / long
term) are **indicative** — the hard constraints are the stated dependencies.

## Delivered to date

The foundation is in place and in daily use: a shared-rayon-pool concurrent crawler that walks a
tree into a Parquet index partitioned by top-level subdirectory (path, size, uid, gid, mode, atime,
mtime, ctime), and the three tools that read it — `xdu-find` for scripted DuckDB queries, `xdu-rm`
for guarded bulk deletion, and `xdu-view` for interactive exploration with both a list view and a
Miller-columns tree view (file-type detection and a scrollable text preview pane). Packaging is
established too: the release tarball, `install.sh`, the scdoc man pages, and generated shell
completions (so GitHub issue #4 is effectively resolved). The tarball also carries the DuckDB
extensions xdu cannot link statically, pinned to its engine version under `lib/xdu/extensions/`, so
nothing downloads at runtime (record in `spec/duckdb-extension-distribution/`). Everything below
builds on that baseline.

---

## S3 as an index target

Indices today live on local disk, which ties them to the machine that built them. The
`<partition>/<chunk>.parquet` layout maps directly onto object-store key prefixes, so writing the
index to S3-compatible storage is cheap — and it unlocks centralized, build-once/read-anywhere
indices that any tool (or a future web client) can point at without copying files around.

Reading is the half worth stating precisely, because it is the half a first cycle got wrong by
staging the whole index to a temp directory per query. DuckDB's `httpfs` reads Parquet over HTTP
range requests: measured against an 80.1 MiB index, `--count` moved 0.13 MiB and a `--top 10` moved
3.94 MiB, where a staging design pays the full 80.1 MiB every time. The intention is that a query
costs what the query asks for, not what the index weighs.

*Horizon: mid-term · Depends on: shipped DuckDB extensions, for `httpfs` — met, on `main` since #31 · Refs: in flight as a pilot (build into a bucket; `xdu-find` and `xdu-view` query in place) — see `spec/s3-index-target/`; its deferrals are the five entries after `S3 as a crawl source` and the debug-build DuckDB entry*
**Seed:** [`issues/s3-index-target.md`](issues/s3-index-target.md)

## S3 as a crawl source

Organizations increasingly park huge datasets in object storage — data lakes, cold archives, tiered
backups — and get none of the size/age/pattern auditing there that xdu gives a POSIX tree. Treating
an S3 bucket as a *source* of file metadata, alongside the local filesystem, brings that same
accounting to object storage. Architecturally this is a larger move than the write path: it means
abstracting the crawler behind a trait so a local jwalk backend and an S3-listing backend are
interchangeable behind one CLI, and expressing per-source capability differences (object storage has
no atime — the Unix-only/no-atime assumption no longer holds for every backend).

*Horizon: long-term · Depends on: crawler-source abstraction; expect dedicated research + sub-phases · Refs: —*
**Seed:** [`issues/s3-crawl-source.md`](issues/s3-crawl-source.md)

## `xdu-rm` against an index in S3

The S3 pilot builds an index into a bucket and lets `xdu-find` and `xdu-view` query it in place, but
`xdu-rm` refuses an `s3://` locator before it reads anything. An operator who keeps one central index
still needs a local copy of it to run a guarded bulk delete. The deletions stay local; what is missing
is reading the index in place under every one of `xdu-rm`'s destructive gates unchanged, which is
high-blast work on the one tool that deletes files.

*Horizon: mid-term · Depends on: S3 as an index target · Refs: `spec/s3-index-target/` (GOAL non-goals, R8)*
**Seed:** [`issues/s3-index-rm.md`](issues/s3-index-rm.md)

## S3 credentials beyond environment variables and the shared files

xdu's one credential resolver reads the `AWS_*` variables and the shared credentials and config files,
and nothing else. Instance metadata, SSO caches, web identity, `credential_process` and assumed roles
are missing, so an operator whose credentials come from one of those must export static keys first.
Any widening has to stay in the one resolver, so the writer and the readers keep resolving the same
identity, and has to settle whether DuckDB's `credential_chain` (the separate `aws` extension) earns a
place in the pinned set.

*Horizon: mid-term · Depends on: S3 as an index target · Refs: `spec/s3-index-target/` (GOAL non-goals)*
**Seed:** [`issues/s3-credential-chain.md`](issues/s3-credential-chain.md)

## S3 traffic through an HTTP proxy

Neither direction of the S3 support uses a proxy: the bundled DuckDB ignores `HTTP(S)_PROXY`, and the
writer's client is built with the proxy turned off so that writer and readers cannot take different
routes. A site that reaches its object store only through a proxy cannot use an S3 index. The fix is
one proxy configuration that reaches both directions.

*Horizon: near-term · Depends on: S3 as an index target · Refs: `spec/s3-index-target/` (PLAN deviation 6)*
**Seed:** [`issues/s3-proxy-support.md`](issues/s3-proxy-support.md)

## Index non-ASCII top-level directories into S3

`xdu -o s3://…` refuses a top-level directory with a non-ASCII name, because the writer's client
percent-encodes such a name in the key and the readers' glob would no longer match it. A local build
of the same tree works. Building keys raw would admit these names, but only after a reader-parity
wire test shows how `httpfs` lists and reports such keys.

*Horizon: near-term · Depends on: S3 as an index target · Refs: `spec/s3-index-target/` (TECH P3, foreman ruling at P3 run B)*
**Seed:** [`issues/s3-non-ascii-partition-names.md`](issues/s3-non-ascii-partition-names.md)

## Bind the index path and partition names in local-index reader SQL

For an S3 index, nothing a locator, a partition name or a directory prefix carries reaches SQL as
text. For a local index all three still do (invariant §5's known exception), so a `'` in an index
path, a partition name or a directory name fails the query with a parser error, and `xdu-view`
cannot list such a directory locally although it can over S3. In `xdu-rm` the text after such a
quote would be SQL. Binding them retires the exception, at the cost of rewriting every local reader
statement, `xdu-rm`'s included.

*Horizon: near-term · Depends on: — · Refs: `spec/s3-index-target/` (GOAL non-goals, the R6 clarification)*
**Seed:** [`issues/local-reader-bound-parameters.md`](issues/local-reader-bound-parameters.md)

## Index layout, declared by the operator

Four entries in sequence, and the sequence is the point: each one makes the next measurable. The
theme is that an index today is laid out exactly one way, decided by the code rather than by the
administrator who knows what their site asks, and recorded nowhere. The end state keeps the simple
case the default and fastest, lets an operator declare something better when they have reason to,
writes that choice into the index so every tool reads it correctly, and — because a layout you can
change afterwards is a decision that can be revised — makes it reversible without re-crawling.

### Chunk clustering and row-group sizing

Every chunk xdu writes is a single Parquet row group, so statistics eliminate whole files and
nothing inside one; records arrive in walk order, which correlates with nothing the tool is usually
asked about. The question this product exists to answer — what has not been touched in two years —
therefore reads every byte of every chunk. Measured on the oldest 5% of an index: sorting the chunk
and sizing its row groups moves 0.65 MiB where today's write moves 5.58, and 1.04 MiB against 10.07
for the accompanying `sum(size)`. Neither half works alone — sorting without row groups buys 1.2x,
and row groups without sorting are slower than today. First because it changes no format, no layout
and no concurrency, and because partition granularity trades directly against row-group granularity:
sequenced later, it would be measured against a baseline leaving 8x unclaimed.

*Horizon: near-term · Depends on: — · Refs: measured during the DuckDB-native S3 read spike, 2026-09-15*
**Seed:** [`issues/chunk-clustering-and-row-groups.md`](issues/chunk-clustering-and-row-groups.md)

### A self-describing index marker

The marker says what version an index is and nothing about how it is shaped, so the layout is
knowledge held in the readers rather than a fact recorded by the writer, and an index shaped
differently fails on a glob that matched nothing — loud, but naming the symptom. The version gate
compounds it: refusing anything that is not exactly the current version is right for a reader
meeting a newer index and wrong for one meeting an older one, which makes every future layout a flag
day in both directions. Neither is a defect while there is one layout; both become defects at two.
The work is a marker vocabulary with two classes in it — keys a reader must understand or refuse,
and keys it may ignore — so that the first cosmetic addition does not hard-fail a reader in the
field.

*Horizon: near-term · Depends on: — · Refs: prerequisite for the two entries below*
**Seed:** [`issues/self-describing-index-marker.md`](issues/self-describing-index-marker.md)

### Opt-in deeper partitioning, declared at crawl time

One partition level, fixed at the top-level subdirectory, is also the unit of scoped re-indexing —
so on a `/scratch/<user>/<project>` tree it is coarser than the operator wants for both jobs.
This is an opt-in second level, not a migration: measured, today's glob scoping beats a partition
predicate for the single-partition case (1 GET against 2), because narrowing the glob narrows the
listing while a predicate only filters the file list after it. What the opt-in buys is predicates
the glob cannot express — 25 files read instead of 120 for an age band, 4 instead of 120 for a
three-partition `IN`. The crawler takes only keys derivable from the path, known before any
`stat`; keys needing file metadata would break single-writer-per-partition, and belong to the entry
below.

*Horizon: mid-term · Depends on: the self-describing marker; sequenced after clustering, whose granularity it trades against · Refs: —*
**Seed:** [`issues/opt-in-deeper-partitioning.md`](issues/opt-in-deeper-partitioning.md)

### `xdu-repack`: rewrite an index into a different layout

Layout is fixed at crawl time and the only way to change it is to crawl again — hours of
metadata-server load on exactly the filesystems where that is the scarce resource. The same gap
makes every index-format change a flag day, since no existing index can be carried forward.
Rewriting needs none of it: the index already holds every value a new layout could sort or partition
on, so a repack reads Parquet and writes Parquet and never touches the filesystem it describes. It
also absorbs the half of partitioning the crawler cannot afford, because metadata-derived keys cost
a batch job over complete data nothing. Measured: a partitioned rewrite is one statement, and the
work is the safety around it — a schema that survives, provenance that is not laundered, and a
failure that leaves no readable-but-wrong index.

*Horizon: mid-term · Depends on: the self-describing marker; S3 as an index target, for the remote side · Refs: —*
**Seed:** [`issues/xdu-repack.md`](issues/xdu-repack.md)

## Bulk operations: `xdu-cp` and `xdu-mv` over a shared select-act engine

First of the bulk-operations theme, whose entries land in file order: copy/move, then archives,
then diffing. `xdu-rm` proved the pattern: select a set with an index query, act on exactly that
set, with dry-run, confirmation, `--safe` re-stat, and deterministic ordering under `--limit`.
This entry lifts that path into one `lib` implementation behind `rm`, `cp`, and `mv` instead of a
fourth copy of `rm`'s `main`, turning "move everything untouched in two years to cold storage"
and "copy this user's stale logs to a staging prefix" into single safe commands — and the staging
copy is the primitive the archive entry builds on. Sequenced after richer search and the schema
work so both tools are born with the final filter surface and columns rather than retrofitted.

*Horizon: mid-term · Depends on: search + schema entries (sequencing) · Refs: #1*
**Seed:** [`issues/bulk-copy-move-xdu-cp-mv.md`](issues/bulk-copy-move-xdu-cp-mv.md)

## Bulk operations: `xdu-tar` slice archives for backup

Second in the theme, on the engine above. There is no way to archive a slice of a tree — "all
files older than X", "everything changed since the last backup" — with the structure preserved.
`xdu-find | xargs tar` inherits every `xargs` failure and assembles ten thousand appends where
tape wants one ordered stream. `xdu-tar` archives exactly the matched set as one reproducible
stream to file or stdout: a full slice today, an incremental slice once the diff entry below
supplies the selection. Staging through the copy entry's semantics versus streaming the archive
directly is the planning decision, weighed on scratch cost against tape behavior.

*Horizon: mid-term · Depends on: copy/move engine above · Refs: #1*
**Seed:** [`issues/xdu-tar-slice-archive.md`](issues/xdu-tar-slice-archive.md)

## Bulk operations: index diffing for incremental selection

Third in the theme, the selection its archive consumer reads. "Changed since the last backup" has
no primitive today: `--newer-than` is a wall-clock atime filter, the wrong clock for a modification
question, and no A-to-B index comparison exists. Diffing two indices of the same root into
added/changed/removed/unchanged on the path key turns incremental backup from a time guess into a
measured difference — and the theme ordering is what makes it reliable, with the mtime/ctime columns
guaranteed present by the schema now delivered on `main` (record in `spec/richer-index-schema/`).

*Horizon: mid-term · Depends on: richer-schema mtime/ctime, delivered on `main`; after tar above · Refs: —*
**Seed:** [`issues/index-diff-incremental-select.md`](issues/index-diff-incremental-select.md)

## Permission-aware, access-scoped queries

Once ownership and permissions are indexed, the index itself becomes a way to leak information: a
non-root user could read sizes and paths they'd never be allowed to `stat` on the live filesystem.
The trap is that this looks like a query-shaping problem and is not — DuckDB has no user model and
no row-level security, so a filter the reader applies is a convenience for display and never a
boundary: anyone who can open the index files reads every row in them. Enforcement has to come from
a process the caller does not control. That resolves into two invariants, each stateable in one
line. **A local index inherits the permissions of whoever crawled it** — nothing to configure, and
what ships today. **A shared index is only ever read through the service** — nothing to configure,
and the seven entries below. The web client is what makes the second unavoidable rather than merely
preferable: a browser has no POSIX identity, so a centrally built index needs an authenticated query
service whether or not anything else does. Together with the v1.0 checkpoint, this stack is the end
state the project is aiming at before a v1.0 cut.

*Horizon: long-term · Depends on: richer index schema (owner/group/perms), delivered on `main`; decomposed into the seven entries below · Refs: #3*
**Seed:** [`issues/access-scoped-queries.md`](issues/access-scoped-queries.md)

## Crawl-time visibility classification

A shared index cannot be scoped per user until it knows who may see each row, and the `uid`/`gid`/
`mode` columns on `main` are the wrong key for it. `xdu` stats rather than opening files, so
visibility is a property of the *directory chain* and not the leaf: a `0600` file under `0755`
directories is fully visible to `du`, and a `0644` file under one `0700` ancestor is invisible.
Resolving that per row at query time means walking ancestors, which is the work the index exists to
avoid. The intent is a top-down closure computed once at crawl time — a directory's class is its
parent's intersected with its own, and files inherit their parent's — running over the directory set
rather than the file set, so it costs little next to the walk. Crawling as root is what makes it
complete rather than merely convenient. The same pass pins a `uid`→name snapshot to the generation,
so an index read months later cannot attribute one person's paths to whoever holds that uid now.

*Horizon: mid-term · Depends on: richer index schema, delivered on `main` · Refs: —*
**Seed:** [`issues/index-visibility-classification.md`](issues/index-visibility-classification.md)

## Shared-index API contract: typed operations, not SQL

`xdu-find` exposes DuckDB SQL, which is right for an index the caller owns and unshippable for a
served one: the dialect includes `read_parquet()`, `ATTACH` and `COPY ... TO`, so caller-written SQL
is arbitrary file read and write as the service account — and with `httpfs` loaded, a route to the
service's own credentials. Appending a scoping predicate does not rescue it, since a CTE or `UNION
ALL` reaches the base relation before any wrapper applies. The intent is a typed, versioned surface
covering what the readers actually need — prefix rollups, top-N, owner/age/size/pattern filters,
histograms — each compiled server-side into SQL the service authored, so the scoping predicate is
structural and every operation has a bounded cost shape. Ad-hoc SQL survives as an operator
endpoint, where the caller could already read the filesystem anyway. It is also the surface the Wasm
client needs, and it can be settled before the server exists.

*Horizon: mid-term · Depends on: — (specifiable ahead of the service) · Refs: —*
**Seed:** [`issues/shared-index-api-contract.md`](issues/shared-index-api-contract.md)

## `xdu-api`: the shared-index query service

An index built as root across a center describes every tenant on it, and nothing today can hand a
tenant only their slice. The intent is a service that is the only reader with access to the store:
it authenticates the caller, resolves them to a set of visibility classes, answers typed API
operations scoped to that set, and records what it answered. The index is read-only, so the service
is stateless — replicas behind a load balancer, no coordination. DuckDB runs hardened and locked
(external access disabled, configuration locked after the index is attached), a posture only
reachable with the index on local disk, which points at object storage as the distribution mechanism
rather than the query path. Aggregates report what the caller can see and count what was withheld: a
true total across invisible rows is a differencing oracle, and a silently filtered one contradicts
`lfs quota`. Serving queries rather than handing out data is also what keeps grants revocable.

*Horizon: long-term · Depends on: visibility classification, the API contract, authentication; S3 as an index target — expect sub-phases · Refs: —*
**Seed:** [`issues/xdu-api-query-service.md`](issues/xdu-api-query-service.md)

## Authentication (`xdu-login`) and group resolution for a shared index

The service can only scope a query if it knows who is asking and which groups they are in, and
neither client can simply tell it — a browser has no POSIX identity at all, and a CLI's uid is not
something a service can trust across a network. Nor does a CLI caller have anywhere to keep proof of
identity between invocations: `xdu-find` runs in loops and from scripts, so a reader that negotiates
credentials on every call is unusable non-interactively, and one that prompts partway through a
query is worse. The intent is one authentication path for both clients — bearer tokens from the
center's identity provider, device-code flow for the CLI, Kerberos where a site already runs it —
fronted on the command line by **`xdu-login`**, which performs the exchange once, stores the token
under the user's config directory with owner-only permissions, and refreshes it on expiry, so the
readers carry no authentication code of their own and `xdu-login --status` answers the first
question anyone debugging a missing row will ask. The two halves of identity pull opposite ways and
cannot share a table: group membership must be current, because someone added to an allocation this
morning expects to see it this morning, while `uid`→name must be pinned to the generation, because
centers recycle uids after account deletion. Membership is precomputed on a bounded TTL rather than
an LDAP query per request, which also makes the exposure window a number a center can publish: crawl
interval plus group TTL.

*Horizon: long-term · Depends on: visibility classification (generation identity snapshot); per-center IdP variation, expect dedicated research · Refs: —*
**Seed:** [`issues/shared-index-authentication.md`](issues/shared-index-authentication.md)

## Client routing to a shared index

`xdu-find`, `xdu-view` and `xdu-rm` take an index path and open it. Once some trees are served
instead, every user would have to know which is which and invoke the tools differently — a
distinction that is an artifact of how the index is stored, not something a user asking about
`/scratch` should have to hold. The intent is a routing table mapping path prefixes to service
endpoints, shipped by config management under `/etc/xdu/` with an environment override for module
files and testing, so the common case is that `xdu-find /scratch/...` simply works. Routing stays a
convenience and never a control: the client is untrusted by construction, so the service
independently validates that a prefix is within its jurisdiction, and paths are canonicalized before
matching so a query cannot be steered at the wrong service.

*Horizon: long-term · Depends on: the API contract · Refs: —*
**Seed:** [`issues/shared-index-client-routing.md`](issues/shared-index-client-routing.md)

## Deployment guards for a shared index

The architecture rests on one premise — that nothing but the service can read the index store — and
as designed that premise is upheld by an operator reading documentation and not making a mistake,
which is exactly the class of failure the served design was chosen to eliminate. The intent is
software that refuses to run misconfigured: `xdu-api` preflights its store at startup and declines
to serve if it is reachable by anyone else, checking provider policy APIs where they exist and
falling back to an anonymous fetch of a known object where they don't, failing closed when the check
cannot be completed. Object names carry no meaning either, since keys named for their visibility
class publish a group-size census to anyone who obtains `List`. The deny-by-default store policy
ships with the project, so a center applies a reviewed configuration instead of composing one.

*Horizon: long-term · Depends on: `xdu-api` (enforces its store-privacy requirement at startup) · Refs: —*
**Seed:** [`issues/shared-index-deployment-guards.md`](issues/shared-index-deployment-guards.md)

## Web client (`xdu-web`)

`xdu-view` is terminal-only, which limits who can explore an index and from where. A Wasm-compiled
progressive web app would be the web equivalent — the same list and tree views, the same search and
filtering — in two modes mirroring the CLI's: a **shared index**, where the app authenticates and
issues typed API operations against `xdu-api`, which scopes every answer to what that user could see
on the live filesystem; and a **local index** the user already holds, queried in-browser with no
service and no configuration. The earlier framing here — browsing an S3-backed index straight from
the browser — was dropped in the 2026-09-11 design pass: reaching the store from a page means either
a world-readable index or credentials the user can read back out of it, and a browser has no POSIX
identity with which to scope anything. That constraint is also what makes the served architecture
unavoidable rather than merely preferable.

*Horizon: long-term · Depends on: the API contract, authentication, `xdu-api`; S3 as an index target for the store · Refs: —*
**Seed:** [`issues/xdu-web-client.md`](issues/xdu-web-client.md)

## Streaming index updates & Lustre changelog

Full re-crawls don't scale to filesystems that change constantly under billions of files — by the
time a crawl finishes it's already stale, and re-running it is enormously expensive. The intent is an
incremental, Iceberg-style **merge-on-read** index: a base snapshot from a full crawl, augmented by
delta files fed from a storage change stream, with periodic compaction folding deltas back into the
base. The primary driver is the Lustre changelog (a modern replacement for Robinhood), but the change
stream should be a pluggable abstraction — with an inotify-backed backend as a general-purpose demo
and community reference. A related open question is whether native Lustre LFS/llapi bindings would
make the full crawl itself faster or gentler on metadata servers than going through the VFS.

*Horizon: long-term · Depends on: — (largest effort on the roadmap; expect several sub-phases) · Refs: —*
**Seed:** [`issues/streaming-index-updates-lustre-changelog.md`](issues/streaming-index-updates-lustre-changelog.md)

## Crawl progress: quiet lines should show elapsed with zero yields

The skewed-tree pass gives a quiet partition evidence of life, but only while its walker yields
entries — the refresh sits inside the entry loop. A partition whose reads block entirely keeps the
bare `scanning...` with no dirs count and no elapsed, so quiet-for-seconds and quiet-for-an-hour
read identically, and a stall across every partition at once freezes the global line too. The
intent is a yield-independent refresh (elapsed-since-activity on each quiet line) that leaves the
single-pool work-stealing walk untouched.

*Horizon: near-term · Depends on: the skewed-tree display fix, delivered on `main` — see `spec/crawl-progress-misleads-on-huge-trees/` · Refs: `spec/crawl-progress-misleads-on-huge-trees/REVIEW.md` (cycle 1, F1)*
**Seed:** [`issues/crawl-progress-zero-yield-stall.md`](issues/crawl-progress-zero-yield-stall.md)

## `man xdu` hyphenates the completion-marker path, so the page operators read is wrong

Fixing the CI gate made `main` green; it did not make the page correct. `man-db` renders with `groff`,
and `groff` hyphenates where `mandoc` does not — so at the default width `man xdu` publishes
`OUTDIR/.xdu-com` + **U+2010** + newline + `plete`, ten such hyphens on that page alone, and an operator
who copy-pastes the marker path gets a filename that cannot exist. It reproduces identically on roff
from both `scdoc` versions, so neither the normalization fix nor pinning a newer `scdoc` touches it,
and CI cannot see it because CI renders with `mandoc`. Notably this **falsifies the premise** recorded
when the question was set aside ("one adversarial run measured zero U+2010") — the boundary was
reasonable, the reason given for it was not. The choice to weigh is whether to teach the gate to
tolerate the hyphen, or to render with `groff` in CI and actually catch the class users are exposed to.

*Horizon: near-term · Depends on: — (independent of the gate fix; the fix does not address it) · Refs: the man-page gate entry above; `spec/manpage-literal-assertion-fails-on-ubuntu/EVIDENCE.md`*
**Seed:** [`issues/manpage-groff-hyphenates-marker-path.md`](issues/manpage-groff-hyphenates-marker-path.md)

## The man-page literal gate is correct but narrow

Four measured gaps in what the literal gate can structurally see, all pre-existing. Three were left
alone because the pass that found them was scoped to making the *existing* assertions
layout-insensitive; the fourth turned out to be an unmet requirement of that pass and was fixed there. No
literal names the binary it belongs to, so copying one rendered page over another is green. The page
list is hard-coded while the render step globs `doc/*.scd`, so a fifth man page is entirely unasserted
— measured shipping the exact historical `OUTDIR//.parquet` corruption past a green gate, which matters
because the bulk-operations theme queues new binaries above. The four env-var assertions are thin
literals — a well-chosen path per page would catch more than a variable name does (their
*duplicate-occurrence* blind spot was an unmet R7 and is already fixed). And
`col -b` rewrites multibyte characters as literal `\xNN` text outside a UTF-8 locale, which bounds what
can ever be asserted and already mis-measured the `groff` work above. Deriving the page list from
`doc/*.scd` is nearly free and converts the widest gap into a build error.

*Horizon: near-term, low priority · Depends on: — · Refs: the two entries above; `issues/ci-gates-are-advisory.md` (a gate that binds nothing is the wider version)*
**Seed:** [`issues/manpage-gate-coverage-gaps.md`](issues/manpage-gate-coverage-gaps.md)

## Piping `xdu-find` into `head` exits 1 with a broken-pipe error

Rust starts with `SIGPIPE` ignored, so when the reader exits early the next
`writeln!` returns EPIPE — and `?` carries it out as `Error: Broken pipe (os
error 32)` with a non-zero exit. Measured: `xdu-find -f csv | head -1` puts
find's own exit at 1, which fails any `pipefail` caller for doing exactly what
the tool invites. Every find output arm shares the locked-stdout loop;
`xdu-rm`'s `println!` paths are the suspected same class with a worse shape (a
panic, not an error), unreproduced at small scale. The fix is its own behavior
contract — silent success on EPIPE, still loud on `/dev/full` — not a rider on
the schema cycle that surfaced it.

*Horizon: near-term · Depends on: — · Refs: —*
**Seed:** [`issues/broken-pipe-closed-stdout.md`](issues/broken-pipe-closed-stdout.md)

## Internal cleanups surfaced by the crawl-hardening pass

The crawl-hardening work produced a wider architecture assessment whose low-risk cleanups were applied
at the time (a shared `lib::index_glob` behind every reader's Parquet glob, one home for the
index-layout constants, reader awareness of the completion marker). It also recorded what was too
risky or too large to fold in: routing the DuckDB injection surface through validated escaping on the
`index_glob` seam, reconciling `xdu-view`'s `format_file_count` with `lib::format_count`, and lifting
the pure TUI helpers — `strip_ansi` above all, which is load-bearing for terminal safety — out of the
2,500-line `xdu-view` into `lib` where they can be tested. Most of these are invisible to users and
decide how much of the codebase stays testable as it grows; the terminal-safety pair tracked separately
below is **not** — a wedged terminal and a filename that crashes the TUI are both user-facing. The full
record, including the performance levers the benchmark work evaluated and rejected, is
[`spec/crawl-hardening/ASSESSMENT.md`](spec/crawl-hardening/ASSESSMENT.md).

*Horizon: near-term · Depends on: — · Refs: —*
**Seed:** [`issues/crawl-hardening-internal-cleanups.md`](issues/crawl-hardening-internal-cleanups.md)

## `xdu-view` terminal safety: panic-safe restore and multibyte truncation

Two invariant §12 gaps in `xdu-view`, both pre-existing and both user-facing. The terminal restore is
plain sequential code after `run_app` with no Drop guard and no panic hook, and `panic = "abort"` means
no unwind would run one anyway — so any panic, or an early `?` from the fallible `Terminal::new` that
already runs after raw mode is entered, leaves the terminal wedged. Separately, both renderers truncate
display names by slicing `&str` at a byte offset computed in terminal columns, which panics outright on
a multibyte filename. The two compound: the second is exactly the panic the first fails to clean up
after. One change to one file, best done together with the `strip_ansi` lift above.

*Horizon: near-term · Depends on: — · Refs: —*
**Seed:** [`issues/xdu-view-terminal-safety.md`](issues/xdu-view-terminal-safety.md)

## Completion marker: scoped runs should not speak for the whole index

`xdu` clears the completion marker on every run — including `xdu -p onepartition` — and rewrites it
from that run's stats alone. So a clean partition-scoped re-index resets `errors=0` and silently retires
the tolerated-error warning that an earlier `--allow-errors` run recorded, while the skipped regions in
other partitions remain missing. The readers, `xdu-rm` included, then report a clean bill of health for
an index that is still incomplete. Needs per-partition attestation, or a scoped run declining to write a
whole-index marker — marker-format or CLI-semantics work either way.

*Horizon: near-term · Depends on: — · Refs: index format versioning, delivered on main*
**Seed:** [`issues/marker-scoped-run-attestation.md`](issues/marker-scoped-run-attestation.md)

## Re-indexing never retires a partition whose source directory is gone

Delete a top-level directory from an indexed tree, re-index, and its partition — chunks and rows — stays
in the index forever: `finalize` prunes stale chunks only *within* the partitions a run actually walked,
so one it never enqueued is never reconciled. The run exits 0 and writes a completion marker, so every
reader reports a clean index that is still answering queries with rows for files that no longer exist —
`xdu-rm` matches them, and a purged project keeps counting against the tree's size forever. The marker's
own `files=` count contradicts the row count the readers return, and nothing compares the two. The stale
partition is **pre-existing behaviour**; what is new is having an attestation that fails to detect it.
Needs whole-index reconciliation (and a scoped run must never delete the partitions it was told to skip),
so it lands next to the scoped-marker work above.

*Horizon: near-term · Depends on: — · Refs: Completion marker scoped runs (same question, marker side)*
**Seed:** [`issues/orphan-partition-survives-reindex.md`](issues/orphan-partition-survives-reindex.md)

## Re-indexing an unreadable partition deletes the rows it already held

The sharpest member of the same family, and the only one that destroys data rather than leaving extra.
When a partition's source directory cannot be read, its walk yields one error and zero files — but the
partition is still finalized, and `finalize` prunes from chunk 0, taking every chunk the previous index
held. The prune loop is correct for the job it was written for (retiring the surplus of a prior larger
run) and simply cannot tell "legitimately smaller now" from "could not be read". With `--allow-errors`
the run then exits 0 and attests itself, which is the cruel case: that flag exists so an operator who
*expects* unreadable regions keeps the rest of the index, and today it can leave them with less than
they started with. The trigger is ordinary on shared storage — a permission change, a stale mount, an
NFS blip. The prune scope is **pre-existing in `main`**, where the same rows vanish with no diagnostic
at all; what this pass added was the first visibility into it. Wants per-partition error state on
`PartitionBuffer` so finalize can decline to prune what it could not read.

*Horizon: near-term · Depends on: — · Refs: the two reconciliation items above — same finalize scope*
**Seed:** [`issues/unreadable-partition-prunes-prior-chunks.md`](issues/unreadable-partition-prunes-prior-chunks.md)

## Benchmark harness: stop `baseline` mode overwriting the committed reference

`bench/run.sh baseline` defaults `--out` to `bench/results/baseline.json`, and `baseline` mode is also
the configuration set anyone reaches for to capture a comparison — so the natural command for "measure
my build against the reference" destroys the reference. It is the one file in `bench/results/` that is
not reproducible on demand: regenerating it yields a *different* baseline, silently redefining what "no
regression" means. A `usage()` warning exists; the loaded default does not.

*Horizon: near-term · Depends on: — · Refs: —*
**Seed:** [`issues/bench-baseline-overwrite-guard.md`](issues/bench-baseline-overwrite-guard.md)

## CI gates are advisory: nothing enforces a red check

The repository has good gates — format, clippy, the test matrix, the man-page literal assertion, a
container build guardrail — and nothing anywhere makes any of them binding. `main` is unprotected with
no rulesets, so a pull request merges over failing checks (PR #10 did, with three), commits reach
`main` directly without a PR at all (ten of the last fifteen), and a batched push means a newly added
gate may never be executed before it lands — which is exactly how the man-page assertion arrived
already broken and stayed that way. Releases do not depend on a green image build either, so three of
them shipped with it red.

This is the reason the other two defects survived rather than a defect in its own right, and it is why
they are being written down instead of quietly fixed: the gap is between "the gate exists" and "the
gate binds anything". The fix is small — a ruleset with required checks and no direct pushes, plus a
scheduled canary for the image build that no PR reliably triggers — but it has a hard ordering
constraint. Enforcement has to land *after* the two red gates are green, or it blocks the very changes
that would fix them.

*Horizon: near-term · Depends on: the man-page gate and container-image fixes above must land first (enforcement would otherwise block its own remediation) · Refs: `spec/readers-autoload-parquet-at-runtime/META.md` F6 covers the factory-skill half*
**Seed:** [`issues/ci-gates-are-advisory.md`](issues/ci-gates-are-advisory.md)

## HPCCM recipe: assert the image carries the shipped extensions

Release tarballs now carry the pinned DuckDB extensions in `lib/xdu/extensions/`, and the HPCCM
recipe puts them beside `bin/` by extracting the whole tarball, but nothing in the recipe checks
it. The repository's own `Dockerfile` and every release leg fail when the extension is missing; the
HPCCM image would build green and fail at its first S3 query. The guard is one `test -f` in the
existing smoke step, and it cannot land early: the recipe pins a release with no `lib/`, so the
guard would break it on `main`. It belongs in the release commit that first moves the recipe's
`VERSION` to a tarball carrying `lib/`.

*Horizon: near-term · Depends on: the first release that ships `lib/xdu/extensions/` · Refs: —*
**Seed:** [`issues/hpccm-extension-guard.md`](issues/hpccm-extension-guard.md)

## The bundled DuckDB does not build against the macOS 27 SDK

The vendored DuckDB 1.4.4 amalgamation fails to compile against the libc++ in the macOS 27
command-line tools SDK: its thrift `TEnumIterator` has no `operator==`, and the error surfaces in
libc++'s `__tree` header. A macOS host builds today only with `SDKROOT` pointed at a 26.x SDK, and
the CI macOS legs pass only because their runner images still ship an older SDK. When those images
move to SDK 27, every macOS leg, the release build included, goes red without a change here.

*Horizon: near-term · Depends on: — (an engine bump re-pins the shipped extensions) · Refs: —*
**Seed:** [`issues/macos-sdk27-bundled-duckdb-build.md`](issues/macos-sdk27-bundled-duckdb-build.md)

## A debug build aborts on one local query inside the bundled DuckDB

A debug build of `xdu-find` aborts with `Assertion failed: (min_val <= input)` in the bundled
engine's `compress_integral.cpp` on one local query shape (partition scope, a size filter and a size
sort over a fixture of distinct sizes); the release build answers it correctly. Tests run debug
binaries, so a test over that shape kills the test binary, and the S3 parity tests avoid the
combination for now.

*Horizon: near-term · Depends on: — (an engine bump re-pins the shipped extensions) · Refs: `spec/s3-index-target/` (TECH P2)*
**Seed:** [`issues/duckdb-debug-build-integral-assertion.md`](issues/duckdb-debug-build-integral-assertion.md)

## Native OS packages (DEB / RPM)

Installation today is a release tarball plus `install.sh`. Native `.deb` and `.rpm` packages would
let HPC and enterprise Linux users install and upgrade xdu through their system package manager,
removing a small but real adoption friction. The existing tarball layout and `install.sh` contract
already define the exact file map these packages would ship.

*Horizon: near-term, low priority · Depends on: — (builds on the established release layout) · Refs: #5*
**Seed:** [`issues/native-os-packages-deb-rpm.md`](issues/native-os-packages-deb-rpm.md)

## Toward v1.0: narrative, branding, and community

A v1.0 cut is as much about explaining the project as shipping code. Why does an extreme-scale
storage indexer deserve attention, how was it built, and how can others contribute? This covers the
non-code work that turns a capable tool into a maintained project: a written motivation-and-
architecture piece, a project identity, a contribution guide and maintenance plan, and a README that
goes beyond basic usage.

*Horizon: long-term · Depends on: — (a release checkpoint, not a feature) · Refs: —*
**Seed:** [`issues/toward-v1-0-release.md`](issues/toward-v1-0-release.md)

## Richer search: glob delivered; fuzzy, full-text, content-type to go

Regex path matching is powerful but not friendly — most users think in globs (`*.py`), not anchored
regex (`\.py$`). The glob half of this entry is delivered on `main`: `-p/--pattern` takes a glob by
default across `xdu-find`, `xdu-view`, and `xdu-rm`, with `--regex` opting back into regular
expressions. What remains is fuzzy matching for approximate filename search and DuckDB's full-text
search extension for richer queries — each now its own seed — alongside the longer-term content-type
filtering ("all video files over 1 GB"), which depends on MIME metadata living in the index and
ties back to the schema work delivered on `main` (record in `spec/richer-index-schema/`).

*Horizon: mid-term · Depends on: content-type filtering needs the richer schema · Refs: —*
**Seeds:** [`issues/fuzzy-filename-matching.md`](issues/fuzzy-filename-matching.md),
[`issues/duckdb-fts-evaluation.md`](issues/duckdb-fts-evaluation.md)
