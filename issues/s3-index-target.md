---
status: shaped on feature/s3-index-target (2026-11-05) — see spec/s3-index-target/GOAL.md
kind: feature
appetite: big
---

# S3 as an index target

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

Indices live on local disk, which ties them to the machine that built them. There is no way to store
an index centrally and point every tool at it.

## Why it was deferred

Not deferred from a factory pass — a forward-looking intention from the original roadmap, recorded
before the `issues/` back-reference convention was enforced. This file retrofits the back-reference;
no new scoping was done.

**Rescoped 2026-09-15.** A first cycle on `feature/s3-index-target` shaped this and built through P2
before the read path was found to be pulling the whole index down to a temp directory per query. The
requirement below is restated so that outcome no longer satisfies it, and the acceptance criteria
now carry the seams the spike opened. See *Notes* for what was measured.

## Outcome / vision

The `<partition>/<chunk>.parquet` layout maps directly onto object-store key prefixes, so `xdu`
writes the index to S3-compatible storage and any tool — or a future web client — points at it
without copying files around. Build-once, read-anywhere indices.

Reading is native, not a download. DuckDB's `httpfs` reads Parquet over HTTP range requests, so a
query fetches footers and the row groups its predicates actually need. That is the property worth
contracting: an operator who asks `--count` against a centrally held index pays for a footer read,
not for the index.

## Sketch of the acceptance criteria

- **R1** — `xdu` SHALL write the partitioned Parquet index to an S3-compatible bucket/prefix instead
  of a local directory, including the reserved `__root__` partition and the run-completion marker
  with its format-version attestation.
- **R2** — WHEN a reader is pointed at an S3-backed index, it SHALL query it in place over ranged
  reads. Bytes transferred SHALL scale with the selectivity of the query, not with the size of the
  index — no whole-index copy to local disk, and no per-query full-object fetch.
- **R3** — `xdu-rm` SHALL apply its existing destructive gates unchanged against an S3-backed index:
  interactive confirm by default, `--dry-run` deletes nothing, `--force` skips the prompt, `--safe`
  re-stats before unlink, and `--limit` carries a deterministic order so a dry run and the real run
  select identical rows.
- **R4** — IF the endpoint is unreachable, credentials are missing or rejected, the index is absent,
  or the marker states a format version the reader does not understand, THEN the tool SHALL fail
  without rows — without deletions, for `xdu-rm` — and report the cause on stderr with a non-zero
  exit, consistent with the local-index posture.
- **R5** — The index location, partition names and credentials SHALL reach DuckDB as bound
  parameters or through its configuration API, never as interpolated SQL text.
- **R6** — WHEN pointed at any S3-compatible endpoint (AWS S3 plus MinIO-style stores) with standard
  credentials and an explicit endpoint override, both directions SHALL work without per-provider
  special-casing visible to the operator.

## Notes

- **The shipped-extensions prerequisite is met, on `main` since #31** (record in
  `spec/duckdb-extension-distribution/`). `httpfs` cannot be statically linked — the vendored
  amalgamation carries sources for `core_functions`, `parquet` and `json` only — so it arrives as a
  shipped binary rather than a runtime download.
- **What the extension-distribution cycle left for this one.** It shipped `httpfs` in every release
  tarball at `lib/xdu/extensions/` and a library loader (`xdu::engine::load_extension`), but no CLI
  path loads an extension yet, so two operator-facing pieces wait for the first one that does, which
  is this cycle's reader:
  - **The `XDU_EXTENSION_DIR` man-page entry.** The variable overrides where the tools look for
    extensions (default `<canonical exe>/../../lib/xdu/extensions`). It is read by the library, not
    declared through clap, so it reaches the `ENVIRONMENT` section of each `doc/*.scd` page whose
    tool loads an extension, with the usual literal-count obligation in the man-page gate.
  - **The `cargo install` caveat.** A `cargo install` build carries no extensions, so an S3 query
    from one fails naming the path it looked in. The README says so today; the man page and the
    error the operator sees should say how to fix it (copy `lib/xdu/extensions/` from a release
    tarball, or set `XDU_EXTENSION_DIR`).
- **R5 is a net reduction in risk, not a new surface.** Invariant §5 records that partition names
  and index paths reach `read_parquet` through raw `format!` today. Measured 2026-09-15:
  `read_parquet(?)` accepts a bound parameter, and S3 credentials can be set through the `duckdb`
  crate's `Config` API without appearing in any SQL string. Native reads let that seam be closed
  rather than routed around.
- **The readers need no new Rust dependency.** DuckDB performs the object-store I/O, and the
  completion marker reads through `read_text('s3://…/.xdu-complete')`. Only the crawler needs a real
  S3 client, because it has to PUT bytes; re-encoding chunks through `COPY` on the crawl hot path
  would not be an acceptable price. One credential resolver in `lib` feeding both is the coherence
  risk to design against.
- **Measured against an 80.1 MiB index, four partitions, through a request-logging proxy:** every
  GET carried a `Range` header and no object was fetched whole. `--count` across all partitions
  moved 0.13 MiB; one partition, 0.03 MiB; `--top 10`, 3.94 MiB; a full `path LIKE` scan, 19.10 MiB.
  A staging design pays 80.1 MiB for each of those.
- **Open at promotion:** the marker read has no DuckDB-side size cap — `read_text`/`read_blob` fetch
  the whole object and `glob()` exposes only `file`, so `MARKER_READ_LIMIT` has no equivalent.
  The POSIX hazard it guards (a FIFO at that path) cannot occur in an object store, but a large
  object can.
- **Open at promotion:** `PROVIDER credential_chain` requires the separate `aws` extension and fails
  without it. Resolving the chain in Rust and passing key, secret and session token into DuckDB's
  config avoids shipping a second binary.
- **The `xdu-api` boundary is no longer settled here.** An earlier note assumed the service must
  hold a local copy because its hardened posture disables external access. That trade belongs to
  [`xdu-api-query-service.md`](xdu-api-query-service.md) and should be decided there, on evidence,
  rather than constrained by this seed.
- **Prior research, already done.** The spike that refutes the staging design is committed at
  [`research/duckdb-native-s3-reads.md`](../research/duckdb-native-s3-reads.md). `/xdu-plan`
  should adopt it as `spec/s3-index-target/research/00-*.md` rather than re-derive it. The
  superseded first cycle is archived at tag `archive/s3-index-target-p2`, whose briefs 01, 02,
  04 and 05 are largely transport-independent and still useful.
- Related: [`s3-crawl-source.md`](s3-crawl-source.md) (the source direction, which reuses this
  cycle's locator parsing and credential resolution),
  [`xdu-api-query-service.md`](xdu-api-query-service.md),
  [`access-scoped-queries.md`](access-scoped-queries.md),
  [`opt-in-deeper-partitioning.md`](opt-in-deeper-partitioning.md) and
  [`chunk-clustering-and-row-groups.md`](chunk-clustering-and-row-groups.md) (both measured during
  the same spike).
- Found by: original roadmap; back-reference retrofitted 2026-09-07; rescoped 2026-09-15 after the
  DuckDB-native read spike.
