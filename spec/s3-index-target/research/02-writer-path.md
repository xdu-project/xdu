# Research 02: the crawl writer's output path for `xdu -o s3://…` (R1, R4, writer half of R5)

Anchors are against HEAD (`3ed22d5` plus spec commits). "Archive" means commit `32ba641`
(tag `archive/s3-index-target-p2`, P1).

## 1. The local write path today

- **`-o` parse:** `src/cli.rs:28-30`, `outdir: PathBuf` (non-UTF-8 paths accepted).
- **Pre-flight in `run()`:** source canonicalize `src/bin/xdu.rs:608-611`; `create_dir_all` and
  canonicalize the outdir `:613-620`, the only destination checks; `--partition` validation
  `:628-639`. Inside `crawl()`: top-level `read_dir` `:86-100`, then `build_work_queue` `:102`
  (`src/crawl.rs:241-325`: reserved names `:260-271`, empty queue `:305`, lossy duplicate `:312-322`).
  **`build_work_queue` is the last check that can reject the run.**
- **Marker clear:** `src/bin/xdu.rs:111` → `clear_completion_marker` (`src/crawl.rs:59-71`, tolerates
  NotFound). **Marker write:** `src/bin/xdu.rs:713`, after the `errors && !allow_errors` bail
  `:692-698`. Body: `src/crawl.rs:76-89`. The log line naming it: `src/bin/xdu.rs:719-728`.
- **Chunk encode:** `PartitionBuffer::flush` (`src/crawl.rs:440-490`) builds one `RecordBatch` and
  runs `File::create(.partial)` + `ArrowWriter` (Snappy) `:477-486`. `ArrowWriter<W: Write + Send>`
  takes a `Vec<u8>` unchanged. One batch gives one row group (default cap 1,048,576 rows), so the
  writer already holds the encoded chunk in memory until `close()`, even with a `File` sink.
- **Finalize:** `src/crawl.rs:493-524`. It renames every `.partial` (`:498-506`), *then* prunes ids
  `num_chunks..` by `exists` probes, **stopping at the first gap** (`:509-523`). The unit test
  `test_finalize_prune_stops_at_first_gap` (`src/crawl.rs:972`) locks the stop. The driver calls it at
  partition end (`src/bin/xdu.rs:477-479`).
- **Memory at `-B 100000`:** builders hold about 64 B per path plus 7×8 B ≈ 12 MB per driver. The
  spike's xdu-shaped fixture (40 objects × 100k rows = 80.1 MiB) gives about **2 MiB per encoded chunk**.

## 2. The archived P1 design, graded

| Piece | What it was | Grade |
|---|---|---|
| `IndexSink` trait in `crawl.rs` (`preflight`, `clear_marker`, `stage_chunk`, `commit_partition`, `write_marker`, `describe`) | Seam where `outdir` flowed; `PartitionBuffer` holds `Arc<dyn IndexSink>` | **Port as is** (high-blast file, human gate) |
| `LocalSink` | Old protocol behind the trait, but the chunk is encoded to `Vec<u8>` then `fs::write(.partial)` | **Port**: same files, same rename and prune |
| `PartitionBuffer::flush` → `ArrowWriter` into a `Vec`, `sink.stage_chunk` | One encode for both sinks | **Port with change**: pass the `Vec<u8>` by value. The archive's `bytes.to_vec()` (s3.rs:338) adds a third copy |
| `S3Sink` over `object_store 0.14` (`aws` feature), owned 2-worker tokio runtime, `block_on` from driver `std::thread`s (never rayon workers) | Sync bridge; the runtime lives only in the module | **Port**. Add the 2 tokio workers and the lazy blocking pool to invariant §7's thread budget |
| `stage_chunk` = one PUT at the **final** key, no `.partial` | Single PUT is atomic; the marker is the run commit | **Port**: R1's "sends each chunk as it finalizes" |
| `commit_partition` = LIST `<prefix>/<partition>/` (filters `alice` vs `alice2`, round-trip-checks `NNNNNN.parquet`), DELETE every id ≥ staged | LIST prune | **Port with change**: prunes past a gap, unlike local. Choose one rule (§4) |
| `preflight` = first page of LIST on the prefix | Endpoint, credentials and bucket in one request | **Port with change**: error classification (below) and a bounded `RetryConfig` |
| `clear_marker` DELETE (NotFound tolerated), `write_marker` PUT | | **Port as is** |
| `parse_index_locator` (`s3://`, case-insensitive; bucket required; prefix trimmed) | | **Port with change**: keep `outdir: PathBuf` and classify on the `OsStr`. The archive switched `-o` to `String`, so clap would reject a non-UTF-8 local `-o`, which **breaks R1's "exactly as today"**. Prefer a lowercase-only `s3://` match: today `S3://x` is a local path |
| `--s3-endpoint`/`--s3-region` flags, `XDU_S3_ENDPOINT`/`XDU_S3_REGION`, `S3Options` | Tool-specific options | **Drop** (R5) |
| `AmazonS3Builder::from_env()` + `DEFAULT_REGION` | object_store's own env reading | **Drop** (R5). See the conflict below |
| Path-style forced whenever an endpoint override is set; `allow_http` when the scheme is `http://` | | **Port the rule into the shared resolver's output**, so DuckDB's `s3_url_style`/`s3_use_ssl` agree with the writer |
| `read_marker`, `read_chunk` | Staging-reader helpers | **Drop** from production. Test-only listing via `list_chunk_ids` stays |
| Error text: one context string, "unreachable endpoint, bad credentials, or missing bucket" | | **Port with change**: name the cause (R4) |

**Conflicts with the new GOAL.**
- **Credential source.** `from_env()` reads every `AWS_*` key object_store knows: endpoint,
  web-identity, container credentials, skip-signature. It has **no shared-file or `AWS_PROFILE`
  support**, per the archived brief `04-rust-s3-client.md`. When no keys are found it falls back to
  **instance metadata**, which R5 excludes and which stalls on a host without it. The writer must build
  with `AmazonS3Builder::new()` and pass only the shared resolver's output: key, secret, optional
  token, region, endpoint and url style. It must error ("no credentials: set `AWS_ACCESS_KEY_ID` … or
  `AWS_PROFILE`") **before any network call**, so the builder never runs its own chain.
- **Staging** (`materialize_index`) arrived only in P2 and nothing above depends on it.

## 3. Memory and local disk

The archive encoded each chunk **entirely in memory, then made a single PUT**. There was no
multipart upload and no streaming. Nothing touches local disk per chunk on the S3 path. A per-chunk
buffer that is freed after its PUT is not "assembling the index on local disk", so it satisfies R1.

Peak memory is about (12 MB of builders + about 2 MiB of encoded row group + about 2 MiB `Vec`) per
driver, roughly 16 MB, against roughly 14 MB locally today. That is about 64 MB at `-j 4` and about
0.5 GB at `-j 32`. It scales linearly with `-B`. A single PUT is fine far below the 5 GiB object
limit.

## 4. §2b over S3, step by step

1. Classify `-o` (no network). Resolve credentials, region and endpoint, and **fail on none**.
2. Source checks, unchanged (`:608-639`).
3. **Pre-flight request:** one LIST v2 on `<prefix>/` (first page). object_store exposes no
   HEAD-bucket call, and a LIST proves the endpoint is reachable, the signature is accepted and the
   bucket exists. An empty listing is a fresh index, not an error. **R4's "prefix absent" must mean a
   reader-side refusal only**, since locally `create_dir_all` creates the outdir. The plan should
   state this. Set a short `RetryConfig`: by default object_store retries for minutes, which delays an
   unreachable-endpoint failure and the tests that check for it.
4. `build_work_queue`, the last rejection, then **DELETE the marker**. The DELETE is the real write
   probe. A read-only credential gets AccessDenied here, while the marker is still in place, so the
   prior attestation survives (R4).
5. Each driver flush makes a PUT `<prefix>/<partition>/NNNNNN.parquet`. The key is visible at once,
   whereas locally it becomes visible at finalize. This is harmless: the marker is gone and readers
   refuse a markerless index.
6. Partition end: LIST the partition, then DELETE ids ≥ n. This runs **after** the new chunks land,
   the same order as local (rename, then prune). S3 and MinIO give read-after-write-consistent LIST.
   **Parity decision:** local stops at the first gap, while the archive's LIST deletes every higher
   id. Deleting everything better serves R1's "no stale chunk". The gap case only comes from a foreign
   or manual edit. Either document the divergence or mirror the gap stop (walk the sorted ids while
   contiguous).
7. After the join and the errors gate, PUT the marker last.

**Partial failure.** A pre-flight or DELETE failure leaves the prefix untouched. A PUT failure
mid-partition leaves new chunks `0..k` over the old tail `k+1..`. Locally the old chunks stay intact
plus `.partial` litter. Both states are unattested and refused by readers, so the two are **at parity
at the attestation level** though not byte-identical on disk, and the next good run converges.
Failures in prune or in the marker PUT are unattested, the same as local. Orphan partitions and
concurrent writers stay out of scope.

## 5. Reserved names, `__root__`, key encoding

- `build_work_queue`'s guard is transport-independent, so `__root__` and `.xdu-complete` are rejected
  exactly as today. Keep the guard unconditional over S3, even though S3 would let the object
  `.xdu-complete` and the prefix `.xdu-complete/…` coexist. `__root__` needs nothing special.
- Non-UTF-8 directory names are already lossy (U+FFFD) in the partition string (`src/bin/xdu.rs:92`,
  `src/crawl.rs:152-165`, collapse guard `:312-322`). Locally the partition directory is written under that lossy name. An S3 key is any
  UTF-8 up to 1024 bytes, so U+FFFD is a valid key. Parity holds.
- **Scare: object_store `Path` percent-encodes.** `ObjectPath::from(&str)` (archive s3.rs:182-190)
  encodes characters it deems unsafe per segment: `%`, `#`, `~`, `[`, `]`, `{`, `}`, `^`, `|`, `\`,
  `"`, `<`, `>`, controls, and `.`/`..` segments. Verify the set against the 0.14 source, which was
  not in the local registry. A partition `a~b` would land as key `a%7Eb`. The readers' `regexp_extract`
  partition column (`src/bin/xdu-find.rs:89`) and `-p a~b` scoping would then disagree with a local
  index, which **breaks R1/R2 parity silently**. Options:
  - Refuse names that do not round-trip (`Path::from(name).as_ref() != name`) with a clear error.
    The check must run before the marker clear, right after `build_work_queue`.
  - Build keys without the encoding.

  Probe early: index partitions named with each of those characters, plus space, `+`, a leading `.`
  and U+FFFD, then compare the S3 LIST against the local directory names and against DuckDB
  `regexp_extract`.

## 6. Tests to mirror and how to assert "same index"

Mirror these `tests/crawl_tests.rs` tests against the loopback endpoint:

- `test_basic_and_per_partition_counts` (:37)
- `test_root_partition_holds_only_loose_files` (:58)
- `test_reindex_prunes_stale_rows` (:213)
- `test_no_partial_files_after_success` (:259): no `.partial` keys
- `test_reserved_*` (:279, :322)
- `test_completion_marker_written_on_success_and_cleared_on_failure` (:372)
- `test_rejected_run_leaves_existing_marker_intact` (:423): add the pre-flight cases (bad secret,
  absent bucket, dead endpoint, no credentials). Each must leave the prior S3 marker byte-identical.
- `test_non_utf8_path_is_counted_and_reported` (:596)
- `test_buffsize_chunking` (:686)
- `test_write_failure_abandons_queued_partitions` (:966): needs a fault-injecting proxy (the spike
  has `bench/spikes/proxy.py`) or a deny policy on `p2/*`
- `test_failing_run_logs_error_record_without_marker` (:1101)
- Also assert that **no secret appears on stderr** (R5).

**R1 equality test:** build the same tree twice, locally and to S3, with `-j 1 -B 2`:
- (a) the S3 key set relative to the prefix equals the local file set relative to the outdir
  (files only, since S3 has no empty directories);
- (b) for each partition, the sorted row multiset read from each chunk with parquet-rs is equal;
- (c) the marker key set and values are equal except `completed_at`.

Avoid comparing chunk bytes: jwalk row order is not guaranteed across runs. Once R2 lands, add
`xdu-find` output equality.

## Change map

| File | Change | R |
|---|---|---|
| `src/cli.rs:28-30` | `-o` help/value_name gains the `s3://` form. Stays `PathBuf`. No S3 flags | R1, R5 |
| `src/crawl.rs` | `IndexSink` + `LocalSink`; `PartitionBuffer` holds the sink, encodes to `Vec<u8>`, passes it by value; local finalize/prune unchanged | R1 |
| `src/s3.rs` (new) | Locator parse on `OsStr`; `S3Sink` (runtime bridge, PUT/LIST-prune/DELETE/marker); builder fed by the shared resolver; error classification; key round-trip guard | R1, R4, R5 |
| resolver (`src/lib.rs` or `src/s3.rs`) | One resolver: env plus shared files plus profile, giving key, secret, token, region, endpoint and url style. Covered by its own brief | R5 |
| `src/bin/xdu.rs:613-620, 641, 102-111, 713, 719-728` | Build the sink, pre-flight before the clear, marker through the sink, `describe()` in the log | R1, R4 |
| `Cargo.toml` | `object_store` (`aws`), `tokio` (`rt-multi-thread`), `futures`, `bytes`. tokio, reqwest, rustls and bytes are already in the lock through duckdb | R1 |
| `tests/s3_write_tests.rs` | §6 mirrors plus the R1 equality test | R1, R4, R10 |
| `.agents/factory/invariants.md` §2, §7 | S3 finalize = PUT at the final key plus LIST prune; thread budget | R1 |
| `doc/xdu.1.scd`, CI literal list | `s3://` locator and credential sources | R9 |

**Size if it ports the archive:** about 450 lines of production code (the archive's 522-line
`s3.rs`, minus about 120 lines of read helpers and about 50 lines of options, plus about 80 lines of
error mapping, the key guard and the resolver wiring) plus about 350 lines of tests. That is about
0.75–1 build day for the writer, excluding the resolver itself and the endpoint harness.

## Top 3 scares

1. **Key encoding.** object_store's `Path` silently percent-encodes partition names with common
   characters, which breaks reader parity on partition names without any error. Probe this first.
2. **Credential-chain leak.** Any use of `from_env()`, or a builder left without static credentials,
   makes the writer resolve differently from the readers and can stall on instance metadata. The
   one-resolver rule has to be enforced at the builder.
3. **Failure latency and classification.** object_store's default retries can turn "endpoint
   unreachable" into minutes. Its error variants for NoSuchBucket on LIST, 401/403 and connect errors
   must be mapped to named causes (R4) without echoing secrets. Verify each against the loopback
   endpoint.
