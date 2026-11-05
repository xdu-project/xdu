# GOAL — Build an index into S3 and query it in place

> **Origin spec.** The *what* and *why* — the locked contract `xdu-review` grades against.
> The *how* lives in [`PLAN.md`](PLAN.md) and [`TECH.md`](TECH.md) (written by `xdu-plan`).
> Keep this at the right altitude: solved and bounded, but not over-specified — leave design
> freedom for the plan. Edit requirements here; do **not** silently drift them during build.

- **slug:** s3-index-target
- **kind:** feature
- **appetite:** big, bounded to about 2–3 working days of build  ·  *a pilot: one write path, two
  readers, native ranged reads. `xdu-rm` against S3 and the wider credential chain are follow-ups
  (Non-goals). Feasibility is settled by the committed spike, so research adopts it rather than
  re-deriving it.*

## Problem

An index lives on the local disk of the machine that built it. There is no way to build it once
into central storage and point every tool, on every host, at the same copy. The
`<partition>/<chunk>.parquet` layout maps onto object-store key prefixes, so S3-compatible storage
is the natural central home.

The value of that home depends on how it is read. A first cycle on this slug (archived at tag
`archive/s3-index-target-p2`) built a read path that pulled the whole index down to a temporary
directory on every query, so a `--count` against an 80.1 MiB index cost 80.1 MiB. The spike that
followed (`research/duckdb-native-s3-reads.md`) measured the alternative: DuckDB's `httpfs` reads
Parquet over HTTP range requests, every GET carried a `Range` header, no object was fetched whole,
and the same `--count` moved 0.13 MiB; one partition 0.03 MiB; `--top 10` 3.94 MiB; a worst-case
`path LIKE` scan 19.10 MiB. A query should cost what it asks for, not what the index weighs. That
property is what this cycle contracts, and a design that copies the index locally in any form fails
it.

The prerequisite has landed. `#31` ships the pinned `httpfs` in every release tarball under
`lib/xdu/extensions/` and adds a loader (`src/engine.rs`: `open_connection` with autoinstall and
autoload off; `load_extension`, honoring `XDU_EXTENSION_DIR`), but no CLI path loads an extension
yet.
This cycle's readers are the first, so two operator-facing items deferred from `#31` land here: the
`XDU_EXTENSION_DIR` entry in the man pages of the tools that load an extension, and telling a
`cargo install` user, in the man page and in the error itself, how to supply the extensions a
`cargo install` build lacks (`README.md:99-101` already says so).

Evidence carried from the seed, confirmed against `3ed22d5`:

- Invariant §5 records that partition names and index paths reach `read_parquet` through raw
  `format!`. The spike showed `read_parquet(?)` accepts a bound parameter and S3 credentials can be
  set through the `duckdb` crate's `Config` API without appearing in any SQL (claims 10, 11), so this
  seam can be closed rather than widened.
- Invariant §2b requires the marker read to be size-capped (`MARKER_READ_LIMIT`). Over S3,
  `read_text`/`read_blob` fetch the whole object and `glob()` exposes only the file name, so the cap
  has no direct DuckDB equivalent. The FIFO hazard the cap guards cannot occur in an object store; a
  large object at the marker key can.
- `PROVIDER credential_chain` requires the separate `aws` extension. `#31`'s gate ruled that
  credentials are resolved in Rust and passed through the `Config` API instead (spike claim 11).
- The readers need no S3 client: DuckDB does their I/O, and the marker reads through
  `read_text('s3://…/.xdu-complete')` (claim 7). The crawler does need one, because it must PUT
  bytes, and re-encoding chunks through `COPY` on the crawl hot path is not an acceptable price. One
  credential resolver feeding both directions is the coherence risk to design against.
- Invariant §2's atomic finalize (`.partial` then same-directory `rename`) has no rename in an object
  store, and the marker ordering of §2b (clear after pre-flight, write only on success) still has to
  hold.

## Outcome / vision

An operator runs `xdu` with an `s3://bucket/prefix` where they would give an output directory, and
the index lands in the bucket chunk by chunk as the crawl finalizes it. Anyone with read access then
points `xdu-find` or `xdu-view` at the same locator, on any host, including an air-gapped one that
can reach its object store, and gets the same answers a local copy would give, paying for footers and
the row groups the query needs. Credentials and the endpoint come from the places S3 tools already
look, so the same invocation works against AWS S3 and against an S3-compatible store. Nothing is
downloaded at run time, and a missing extension or a missing credential says what to do about it.

## Acceptance criteria (the contract)

- **R1** — WHEN `xdu` is given an `s3://bucket/prefix` output locator, it SHALL write the same index
  a local run writes (every partition including the reserved `__root__`, the same chunk naming, and
  the completion marker with its format-version attestation) to that prefix, sending each chunk to
  the bucket as it finalizes and never assembling the index on local disk. The §2b ordering SHALL
  hold: an existing marker is removed only after pre-flight passes, including reaching the
  destination with accepted credentials, and the marker is written only after every partition has
  finalized. A re-run into the same prefix SHALL leave no stale chunk in any partition it wrote, as
  a local re-run does. Any argument that is not an `s3://` locator SHALL keep meaning a local path
  exactly as today.
- **R2** — WHEN `xdu-find` or `xdu-view` is given an `s3://bucket/prefix` index locator, through
  `-i` or `XDU_INDEX`, it SHALL query the index in place through DuckDB's native ranged reads and
  return the same results it returns for the same index on local disk, across filters, `--count`,
  `--top`, sorting and partition scoping. Neither tool SHALL copy the index or a chunk object to
  local disk, or fetch a chunk object whole in place of the ranges the query needs. Both SHALL
  apply the existing version gate and completion warning (§2b), and the marker read SHALL stay
  bounded: a marker object larger than the cap is refused without being fetched whole.
- **R3** — Every request a query issues for a chunk object SHALL be a ranged read, and bytes
  received SHALL scale with what the query selects, not with the index's size: on a test index of
  at least [NEEDS CLARIFICATION: fixture floor — proposed 10 MiB] stored across several partitions,
  `xdu-find --count` over all partitions SHALL receive under [NEEDS CLARIFICATION: ceiling —
  proposed 5%] of the index's stored bytes. A test SHALL measure this on every CI run, so a
  regression to a staging read fails the build.
- **R4** — IF the endpoint is unreachable, credentials are missing or rejected, the bucket or prefix
  is absent, or (for a reader) the marker is missing or states a format version the reader does not
  understand, THEN the tool SHALL exit non-zero with the cause on stderr and SHALL emit no rows. For
  `xdu`, such a failure SHALL write no marker, and one detected before the marker is removed SHALL
  leave an existing index and its marker as they were.
- **R5** — Credentials, region and endpoint SHALL be resolved from [NEEDS CLARIFICATION: which
  standard sources the pilot honors — proposed: the `AWS_*` environment variables and the shared
  credentials/config files with profile selection], by one resolver in the library that serves
  both the writer and the readers. An explicit endpoint override SHALL let the same invocation
  target AWS S3 or an S3-compatible store with no provider-specific option visible to the operator.
  No secret SHALL appear in SQL text, on the command line the tools require, or in any output.
- **R6** — In `xdu-find` and `xdu-view`, for local and S3 indices alike, the index locator and
  partition names SHALL reach DuckDB as bound parameters, and credentials through its configuration
  API, never as text interpolated into SQL. This retires invariant §5's raw-`format!` note for those
  two readers.
- **R7** — WHEN a reader opens an S3 index, it SHALL load the pinned `httpfs` through `#31`'s loader,
  from the install location or `XDU_EXTENSION_DIR`, with no download and no write under `$HOME`. IF
  the extension is absent, THEN the tool SHALL fail naming the path it looked in and both remedies:
  copy `lib/xdu/extensions/` from a release tarball, or set `XDU_EXTENSION_DIR`. A local-index query
  SHALL still load no extension, and `tests/offline_tests.rs` SHALL keep passing with its cold `HOME`
  empty.
- **R8** — IF `xdu-rm` is given an `s3://` index locator, THEN it SHALL refuse before any query or
  deletion, saying that S3 indices are not yet supported by `xdu-rm`, and exit non-zero.
- **R9** — The man pages of `xdu`, `xdu-find` and `xdu-view` SHALL document the `s3://` locator and
  the credential, region and endpoint sources R5 honors; those of `xdu-find` and `xdu-view` SHALL
  carry `XDU_EXTENSION_DIR` in `ENVIRONMENT` and the `cargo install` remedy, under the man-page
  gate's literal-count obligation. `README.md` SHALL show building an index into a bucket and
  querying it in place.
- **R10** — The S3 behavior in R1–R8 SHALL be exercised by automated tests against a local
  S3-compatible endpoint on loopback, needing no cloud account, no container runtime, and no network
  beyond loopback. CI SHALL provide that endpoint so the tests run there; where a developer machine
  lacks it, the tests SHALL report themselves skipped, never pass silently.

## Non-goals (no-gos)

- **`xdu-rm` against an S3 index.** R8 refuses it. Applying `xdu-rm`'s destructive gates unchanged
  over an S3 index (the seed's draft R3) is a follow-up seed, filed as `issues/s3-index-rm.md` with
  a `ROADMAP.md` entry when this cycle publishes.
- **Credential sources beyond R5's set**, such as instance metadata, SSO, web identity and
  `credential_process`, and shipping the `aws` extension for `credential_chain`. A follow-up seed is
  filed at publish if the demo or an operator needs one. Adding `aws` to the pinned set stays a
  one-entry change (`#31` R1).
- **Any local copy of index objects**: staging, a per-query temp directory, or a persistent cache
  across queries. Ranged reads are the design (R2, R3).
- **Uploading an existing local index** to a bucket, or syncing one index between locations. An
  operator can do that with standard S3 tools; `xdu` builds into the bucket directly.
- **Atomicity beyond local parity.** Finalize stays per-chunk, not per-partition (invariant §2's
  known limitation). Concurrent writers to one prefix, object locking and bucket versioning are out.
  Orphan partitions stay with `issues/orphan-partition-survives-reindex.md`, and scoped-run marker
  attestation with `issues/marker-scoped-run-attestation.md`.
- **S3 as a crawl source** — `issues/s3-crawl-source.md`, which reuses this cycle's locator parsing
  and credential resolver.
- **The query service and web client** — `issues/xdu-api-query-service.md` and
  `issues/xdu-web-client.md`, which decide their own S3 posture on evidence.
- **Layout changes measured by the same spike**: Hive-style partition directories, row groups and
  clustering, deeper partitioning — `issues/chunk-clustering-and-row-groups.md`,
  `issues/opt-in-deeper-partitioning.md`.
- **Other object stores and transports** (GCS and Azure native APIs, plain HTTP indices).
- **Throughput tuning** of the write or read path beyond what R3 requires.
- **Bumping `duckdb`**, changing the pinned extension set (unless the R5 clarification adds `aws`),
  or changing the release tarball layout.
- **A CI job against a real cloud account.** See the clarification on verification.

## Clarifications

- **Q:** Pilot or the whole seed? — **A:** Pilot. Build into a bucket, query in place with
  `xdu-find` and `xdu-view`, native ranged reads, standard credentials, air-gap-capable install.
  `xdu-rm` over S3 and the wider credential chain are follow-ups (Non-goals). Set by the foreman's
  brief for this cycle (resolved 2026-11-05).
- **Q:** Do the `XDU_EXTENSION_DIR` man-page entry and the `cargo install` caveat deferred from `#31`
  land here? — **A:** Yes, with the first CLI consumer of the loader (R7, R9). Recorded in the seed
  (resolved 2026-11-05).
- **Q:** Which credential, region and endpoint sources must the pilot honor? —
  [NEEDS CLARIFICATION: (a) the `AWS_*` environment variables only; (b) (a) plus the shared
  credentials and config files with `AWS_PROFILE` selection, endpoint included; (c) the full AWS
  default chain, including instance metadata and SSO. Proposed: (b), resolved in Rust, no `aws`
  extension. (c) means shipping `aws` or a Rust SDK chain, which the appetite does not cover.]
- **Q:** What are R3's fixture floor and byte ceiling? — [NEEDS CLARIFICATION: the spike measured
  0.16% for `--count` on 80.1 MiB over 4 partitions, about 33 KiB per chunk object, so the ratio
  depends on chunk count more than index size. Candidates: (a) floor 10 MiB, ceiling 5%; (b) floor
  50 MiB, ceiling 1%, closer to the spike but slower to build in CI; (c) an absolute ceiling per
  chunk object read (say 64 KiB per object), independent of fixture size. Proposed: (a).]
- **Q:** Beyond the local endpoint, must the pilot be verified against a real S3 provider? —
  [NEEDS CLARIFICATION: (a) no, the loopback endpoint in R10 is the whole acceptance; (b) (a) for
  the contract, plus a manual run against a real provider recorded in the PR before merge, as demo
  preparation outside the review gate; (c) a CI job against a cloud account, with secrets. Proposed:
  (b).]

## Related materials

- Seed: `issues/s3-index-target.md` (pre-shaped Problem and draft R-IDs, taken as input).
- Evidence: `research/duckdb-native-s3-reads.md` (claims 1–13, the cost model, the open items);
  probes under `bench/spikes/` (MinIO plus a request-logging proxy). `/xdu-plan` adopts the note as
  `spec/s3-index-target/research/00-*.md` rather than re-deriving it.
- Superseded first cycle: tag `archive/s3-index-target-p2`; its briefs 01, 02, 04 and 05 are
  largely transport-independent.
- Prerequisite: `spec/duckdb-extension-distribution/GOAL.md` (`#31`), notably its clarification
  that credentials go through the `Config` API, not the `aws` extension.
- Anchors: `src/engine.rs` (`open_connection`, `extension_dir`, `load_extension`,
  `EXTENSION_DIR_ENV`); `src/cli.rs:29-30` (`xdu -o`, a `PathBuf` today) and the readers'
  `-i`/`XDU_INDEX`; `.agents/factory/invariants.md` §2, §2b, §3, §4, §5; `tests/offline_tests.rs`;
  `doc/*.1.scd` `ENVIRONMENT` sections; `README.md:91-101`.
- Follow-on consumers: `issues/s3-crawl-source.md`, `issues/xdu-api-query-service.md`.
