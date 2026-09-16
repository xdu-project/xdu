# DuckDB-native S3 reads — the spike `research/03` deferred

> **Pre-cycle evidence, not a design.** Backs
> [`issues/s3-index-target.md`](../issues/s3-index-target.md) and
> [`issues/duckdb-extension-distribution.md`](../issues/duckdb-extension-distribution.md). When that
> seed is promoted, `/xdu-plan` should adopt this as `spec/s3-index-target/research/00-*.md` rather
> than re-derive it; it is written to that brief's shape for exactly that reason.

Run 2026-09-15 on darwin/arm64, toolchain per `rust-toolchain.toml`, `duckdb` crate 1.4.4
(`bundled`, `parquet`), MinIO on 127.0.0.1:19000, with a logging HTTP proxy on :19001 attributing
every request to the query that issued it. The instruments are committed at
[`bench/spikes/`](../bench/spikes/README.md), which maps each probe to the claims below; every
number here is re-runnable from there.

A first cycle on `feature/s3-index-target` built a staging read path on the verdict in its own
`research/03-duckdb-s3-capability.md`, which closed by naming the deciding experiment as open work
for the plan phase and, in the same document, ranking against the option that experiment would have
decided. The spike was never run. The branch is archived at tag `archive/s3-index-target-p2`; its
briefs `01`, `02`, `04` and `05` are largely transport-independent and still useful.

## Confirmed from that brief

- No `httpfs` feature exists on `duckdb`/`libduckdb-sys` 1.4.4.
- The vendored amalgamation carries sources for `core_functions`, `parquet` and `json` only
  (`tar -xzOf duckdb.tar.gz duckdb/manifest.json`). Static linking of `httpfs` is impossible without
  leaving the crate's public surface.

## Contradicted by measurement

| # | Claim under test | Result |
|---|---|---|
| 1 | Bundled static engine loads the official prebuilt `httpfs` | **Yes.** `version()`=v1.4.4, `PRAGMA platform`=osx_arm64 |
| 2 | Extension install redirects off `$HOME` | **Yes.** `SET extension_directory='<dir>'` honored |
| 3 | Pre-seeded dir + `LOAD` works with no network | **Yes.** Cold `HOME` stayed empty, 0 entries |
| 4 | A vendored binary loads by absolute path | **Yes.** `LOAD '<abs>/httpfs.duckdb_extension'` |
| 5 | Missing extension with autoinstall off fails cleanly | **Yes.** `IO Error: Extension "<exact path>" not found.` |
| 6 | A real xdu index in S3 answers the readers' existing SQL | **Yes.** 1801 rows local == 1801 over `s3://` |
| 7 | Completion marker readable without a Rust S3 client | **Yes.** `read_text('s3://…/.xdu-complete')`, `format=2` parses |
| 8 | `xdu-view`'s partition regexp survives `s3://` | **Yes.** yields `__root__,carol,alice,bob` |
| 9 | Reads are ranged, not whole-object | **Yes.** Every GET carried `Range`; no object fetched whole |
| 10 | `read_parquet(?)` accepts a bound parameter | **Yes.** Prepared `read_parquet(?) WHERE size > ?` |
| 11 | Credentials can bypass SQL entirely | **Yes.** `Config::with("s3_access_key_id", …)` then `open_in_memory_with_flags` |
| 12 | `COPY … TO 's3://…'` writes the layout | **Yes.** 4 partitions x 1M rows straight to S3 keys |
| 13 | Official `linux_amd64` `httpfs` respects the shipped glibc floor | **Yes.** Max symbol `GLIBC_2.28`; 24 MiB raw, 8.8 MiB gzipped |

**Claim 13 is inference, not observation.** The ELF's symbol requirements were read; the extension
was never actually `LOAD`ed inside a manylinux container against a bundled-built `xdu`. That is the
one leg to close before committing to this direction.

## Cost model — 80.1 MiB index, 4 partitions, one process per query

| query | LIST | GET | MiB received | vs. staging's 80.1 MiB |
|---|---|---|---|---|
| `count(*)` all partitions | 2 | 4 | **0.13** | 600x less |
| `count(*)` one partition | 2 | 1 | **0.03** | 2700x less |
| `LIMIT 5`, no order | 2 | 1 | **0.03** | 2700x less |
| `size > …` (selective) | 2 | 13 | **3.94** | 20x less |
| `--top 10` by size | 2 | 13 | **3.94** | 20x less |
| `sum(size)` all | 2 | 40 | **15.40** | 5x less |
| `path LIKE` (worst case) | 2 | 40 | **19.10** | 4x less |

Staging pays 80.1 MiB on every one of these.

## Layout findings, which became their own seeds

`<index>/<partition>/NNNNNN.parquet` is **not** Hive-partitioned. `hive_partitioning=true` over bare
directory names produces only the eight schema columns; the same rows under `partition=<name>/` do
yield a `partition` column and filter correctly. Measured on a 120-prefix index: glob scoping beats
a partition predicate for the single-partition case (1 GET against 2, 0.02 MiB against 0.09), while
a predicate the glob cannot express reads 25 files instead of 120. LIST cost was flat at 2 calls for
both 4 and 120 prefixes, so the cardinality limit is the small-file problem, not listing.

Every chunk `xdu` writes is a single row group (`WriterProperties` sets compression only;
`parquet` defaults to 1,048,576 rows, `--buffsize` to 100,000). Selecting the oldest 5% by `atime`:

| chunk written as | GET | MiB |
|---|---|---|
| walk order, one row group (today) | 20 | 5.58 |
| sorted by `atime`, one row group | 20 | 4.79 |
| walk order, ten row groups | 110 | 5.66 |
| sorted by `atime`, ten row groups | 20 | **0.65** |

## Open items

- The marker read is unbounded over S3: `read_text`/`read_blob` fetch the whole object and `glob()`
  exposes only `file`, so `MARKER_READ_LIMIT` has no equivalent.
- `PROVIDER credential_chain` needs the separate `aws` extension and fails without it. Resolving the
  chain in Rust and passing key, secret and session token into DuckDB's config avoids a second
  binary.
- The writer needs a real S3 client to PUT bytes; the readers need none. One credential resolver in
  `lib` feeding both is the coherence risk.
- A repack via `COPY … PARTITION_BY` strips the partition keys from the written files, names them
  `data_0.parquet`, and will carry a `filename` column through a `SELECT *`.
