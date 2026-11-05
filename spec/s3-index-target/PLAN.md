# PLAN — Build an index into S3 and query it in place

> **Status:** Passed the PLAN gate, reconciled with GOAL amendment `0f0edbd` · **Last updated:**
> 2026-11-05
> **Authoritative technical design.** The *how*. Vision/contract is [`GOAL.md`](GOAL.md);
> the phased executable roadmap is [`TECH.md`](TECH.md). Backing detail is in
> [`research/`](research/); decisions are consolidated in
> [`research/00-digest.md`](research/00-digest.md) (D1–D10). Every design element traces to a GOAL
> R-ID.

## 1. Summary

Readers parse `-i`/`XDU_INDEX` into a locator once. For an `s3://` locator they open a DuckDB
connection, load the pinned httpfs through `#31`'s loader, hand it the resolver's credentials as
bound parameters of one `CREATE SECRET`, and run today's SQL with the `read_parquet` glob bound. No
byte of the index lands on local disk, and `--count` costs footers (D1). The writer keeps the
arrow-rs encoder, holds each chunk in memory, and PUTs it with object_store on ring. It keeps §2b's
ordering with a DELETE and a PUT of the marker (D3, D9). One hand-rolled resolver serves both
directions (D4). Tests run an in-process s3s-fs endpoint on loopback behind a counting proxy (D6,
D7). Five phases, ~3 build days, ordered so `xdu-find` over S3 is demonstrable after P2 and
`xdu-view` is a clean cut at P4.

## 2. Design

### 2.1 Locator (`src/s3.rs`, new; re-exported through `lib`)

`enum IndexLocator { Local(PathBuf), S3(S3Locator) }` with `S3Locator { bucket, prefix }`. A raw
argument is S3 only if its bytes start with lowercase `s3://`. Anything else is local and is handled
exactly as today, non-UTF-8 included: `-o` and `-i` stay `PathBuf` in `src/cli.rs`, and
classification runs on the `OsStr` (the archive's switch to `String` broke non-UTF-8 local paths).
Parsing refuses an empty bucket, a `user:secret@` userinfo form, and glob metacharacters
(`*?[]{}`) in the bucket or prefix. It also normalises a trailing `/`, since `prefix//*/*.parquet`
is a different key. `Display` renders `s3://bucket/prefix` and never carries credentials. One
`index_glob_s3(&S3Locator, Option<&str>)` sits beside `lib::index_glob` and shares its layout tail,
so §3's "one place" stays one function per transport.

### 2.2 Resolver (`src/s3.rs`)

`S3Settings::resolve(&EnvSource) -> Result<S3Settings>` returns `{ access_key_id, secret_access_key,
session_token: Option, region, endpoint: Option<Url>, path_style: bool }`. Precedence follows D4.
The INI reader is hand-rolled and covers `[name]` and `[profile name]` sections and the indented
`services` sub-section form. `EnvSource` is a trait over env lookups and file reads, so unit tests
feed fixture files without touching process env. It fails closed on half-set env credentials, on
no credentials, and on an endpoint with a path or a non-`http(s)` scheme. `Debug` redacts the
secret and the token. Every error names the source (variable or file and profile), never a value.
It is the one resolver, and both the writer and the readers consume its output.

### 2.3 Reader connection (`src/engine.rs`)

`open_connection()` stays byte-identical. Local paths call only it, so a local query loads no
extension and `tests/offline_tests.rs` passes untouched (R7). New
`open_s3_connection(&S3Settings) -> Result<Connection>` runs `open_connection()`, then
`load_extension(&conn, "httpfs")`, then one prepared statement:
`CREATE SECRET xdu_s3 (TYPE s3, PROVIDER config, KEY_ID ?, SECRET ?, REGION ?, URL_STYLE ?,
USE_SSL ?[, SESSION_TOKEN ?][, ENDPOINT ?])`. Every value is bound, and the optional clauses are
fixed text chosen by presence (D2). The R7 message at `engine.rs:115-121` gains the release-tarball
remedy: copy `lib/xdu/extensions/` from a release tarball, or set `XDU_EXTENSION_DIR`. It keeps the
substrings `assert_names_absent_file` checks. This statement joins invariant §5's engine surface.

### 2.4 Reader SQL binding (`src/lib.rs`, `src/bin/xdu-find.rs`)

`parquet_source(&IndexLocator, partition, filename: bool) -> (String, Vec<String>)`. For a local
locator it returns today's `read_parquet('<index_glob>'[, filename=true])` with no params, so the
SQL is byte-identical and a unit test pins the strings. For S3 it returns `read_parquet(?[,
filename=true])` with `[glob]`. Each of `xdu-find`'s 7 statements swaps its literal for the source
text and `query([])` for `query(params_from_iter(&params))`. Filters, `--count`, `--top`, sorting
and `-u` are unchanged (`01` §2: the locator and `-u` reach SQL only inside the glob). Invariant
§5's raw-`format!` note for local paths stands (R6).

### 2.5 Marker over S3 (`src/lib.rs`)

`read_completion_marker_s3(&Connection, &S3Locator) -> Result<MarkerRead>` (D8):

1. `SELECT count(*) FROM glob(?)` on the prefix. A failure here is an endpoint or credential error
   and is reported as such (R4).
2. `SELECT size FROM read_blob(?)` on the marker key. A missing key gives `Absent`, and a size above
   `MARKER_READ_LIMIT` gives `Unreadable`, without a GET.
3. `SELECT content FROM read_blob(?)`.

`index_version_error` and `index_completion_warning` take a pre-read `MarkerRead` plus a display
string, so the marker is read once per run. The local path keeps its `fs` read and its FIFO guard.
One `classify_s3_error(&duckdb::Error) -> S3Cause` maps the measured texts (403, `Could not
establish connection`, `No files found`, the 404 that says `Unable to connect`) to named causes. It
falls back to the DuckDB text verbatim, which has been observed not to echo the secret.

### 2.6 Reader flow (`src/bin/xdu-find.rs`, later `xdu-view.rs`)

The order is parse locator, then branch:

- **Local:** today's code.
- **S3:** resolve settings, `open_s3_connection` (R7's error first), read the marker, version gate,
  completion warning, then the query.

No rows are emitted before the gate. A failure exits non-zero with the cause on stderr (R4).

### 2.7 `xdu-rm` (`src/bin/xdu-rm.rs`)

Right after argument parsing (`:26-27`), before the rayon pool, `canonicalize`, any gate or query,
an `s3://` locator exits non-zero with `xdu-rm: S3 indices are not yet supported by xdu-rm`. No
other change in this high-blast file (R8).

### 2.8 Writer (`src/crawl.rs`, `src/s3.rs`, `src/bin/xdu.rs`)

The archive's seam ports from `32ba641` with D9's changes. `trait IndexSink` has
`put_chunk(partition, id, Vec<u8>)`, `finalize_partition(partition, count)`, `clear_marker()` and
`write_marker(body)`. `LocalSink` keeps the `.partial`, same-directory rename, first-gap prune and
marker file byte-for-byte. `PartitionBuffer::flush` encodes once, through `ArrowWriter` into a
`Vec<u8>`, and passes it by value.

`S3Sink` owns a private multi-thread tokio runtime. Driver threads `block_on` it, and rayon workers
never touch it (invariant §7's thread budget gains the runtime's workers). The rest of the writer:

- **Keys and prune:** each chunk is one PUT at its final key `<prefix>/<partition>/NNNNNN.parquet`.
  Finalize is a LIST of `<prefix>/<partition>/` and a DELETE of every higher id.
- **Marker:** a PUT of the same body `LocalSink` writes.
- **Client:** object_store is built from `AmazonS3Builder::new()` with only resolver output. It
  uses the ring provider (installed once), a `no_proxy()` reqwest connector, and a bounded retry
  and timeouts (D3, D5).

`xdu.rs` sequence for an S3 target:

1. Parse the locator.
2. Resolve the settings.
3. LIST the first page of the prefix. This reaches the bucket with accepted credentials, and an
   empty listing is a fresh index.
4. `build_work_queue` and the reserved-name checks.
5. Refuse partition names that will not survive as an S3 key and a DuckDB glob.
6. DELETE the marker. It is the first write, so read-only credentials fail here with the marker
   intact.
7. Run the drivers.
8. Write the marker on the success path only.

The local sequence is unchanged (R1, R4).

### 2.9 xdu-view (`src/view_query.rs`, new; `src/bin/xdu-view.rs`)

The 4 distinct queries move into a lib module (`01` §3), along with `DirEntry`. Those are
`query_root_files`, `load_partitions` ≡ `make_partition_column`, `discover_partition_root`, and
`load_directory` ≡ `make_directory_column_tree`. Each takes `(&Connection, &IndexLocator, …,
&QueryFilters, SortMode)`, and the `App` methods become one-line callers. `App` holds an
`IndexLocator`, and the S3 prologue mirrors §2.6. The file preview opens the *source* path, as it
does today. It is not the index and needs no change. The data-derived `path LIKE '<prefix>%'`
surface is bound on the S3 arm too, since it costs one more `?`. This pays down part of §11's
`xdu-view.rs` debt and is what makes R2 testable for the viewer without a TUI.

### 2.10 Tests and CI (`tests/common/s3.rs`, `tests/s3_tests.rs`, `.github/workflows/test.yaml`)

- **`TestS3` fixture:** an in-process s3s-fs endpoint on `127.0.0.1:0` with SigV4 auth and a temp
  root, plus a std-only counting proxy and hermetic child environment. Children have all `AWS_*`
  and `*_PROXY` removed, a temp `HOME`, fixture credential and config files, and
  `XDU_EXTENSION_DIR` set from `XDU_TEST_EXTENSION_DIR` (D6, D7, D10).
- **Seeding:** the fixture seeds an index by copying a local one into the endpoint's root. It
  compares by walking that root.
- **Wire tests:** the tests that need httpfs are `#[ignore = "needs the pinned httpfs …"]`. Every
  one goes through the proxy and asserts no whole-object GET and no local write (empty `HOME`,
  `TMPDIR` and cwd). They are graded by [`verify/wire.sh`](verify/wire.sh), which is written at plan
  time and passes only when the passed count equals the listed count. Its rules, and the rest of the
  gate hardening, come from [`research/06`](research/06-adversarial-review.md).
- **CI, both legs:**
  - build the debug `extension-probe`;
  - run `packaging/extensions/fetch.sh` into `$RUNNER_TEMP/xdu-ext`;
  - run `cargo test --locked --test s3_tests -- --include-ignored` with `XDU_TEST_EXTENSION_DIR`
    and `XDU_TEST_S3_REQUIRED=1` set, then a guard: passed count equal to the `--list` count
    (`verify/wire.sh`).
- **Developer machine:** without the extension, the wire tests show as ignored. Run with
  `--include-ignored` and the extension missing, they panic.
- **Dev-dependencies:** `s3s = "=0.17.0"`, `s3s-fs = "=0.17.0"`, `hyper-util`, and `tokio`
  (dev-only, so the release build is unaffected).

### 2.11 Docs

Each phase updates the page for the binary it touches, in the same commit (§10). The CI literal
list and `AGENTS.md`'s duplicated-literal sentence change in that same commit (§13's three-place
rule).

- **`doc/xdu-find.1.scd` (P2):** `s3://BUCKET/PREFIX` under `-i`, an `S3 INDICES` paragraph, the
  ten `AWS_*` names in `ENVIRONMENT`, and `XDU_EXTENSION_DIR` with the `cargo install` remedy.
- **`doc/xdu.1.scd` (P3):** `-o` and the same sources.
- **`doc/xdu-view.1.scd` (P4):** as for xdu-find.
- **`doc/xdu-rm.1.scd` (P1):** one sentence that `s3://` is refused. R9 does not require it, but
  `XDU_INDEX` is shared across the tools.
- **`README.md`:** a new `### Indexes in S3` section before `### Querying with DuckDB`. The query
  half lands in P2 and the build half in P3. P2 also adds one clause at `:99-101` saying S3
  indices need `httpfs`.
- **Proxies:** the pages state that proxy variables are not used (D5).

### Requirement → design map

| R-ID | Design element(s) that satisfy it |
|------|-----------------------------------|
| R1 | §2.8 `IndexSink`/`S3Sink`, PUT per chunk at finalize, LIST-prune, §2b sequence; unsafe top-level names refused before the marker clear, naming the directory; `-o` stays `PathBuf` with `OsStr` classification; round-trip test compares the endpoint root to a local run, and a run with empty `TMPDIR`/`HOME` leaves both empty (P3) |
| R2 | §2.4 binding + §2.6 flow for `xdu-find` (P2); §2.9 for `xdu-view` (P4); §2.5 marker cap; local-vs-S3 parity tests across filters, `--count`, `--top`, sort, `-u` |
| R3 | D7 counting proxy; ≥10 MiB, ≤12-chunk fixture over 4 partitions; ranged-only and <5% assertions with a negative control; runs in CI's S3 step every push (P2) |
| R4 | §2.5 `classify_s3_error` + glob-then-HEAD marker read (readers, P2); §2.8 pre-flight LIST before the marker DELETE, no marker on failure, an absent or empty prefix is a new index (writer, P3); test matrix: dead port, wrong secret, no credentials, missing bucket, missing prefix/marker, unknown `format` |
| R5 | §2.2 resolver in `src/s3.rs` serving `S3Sink` and `open_s3_connection`; no `aws` extension; secret only in bound parameters, redacted `Debug`; tests assert the secret is absent from stdout/stderr, from `duckdb_settings()` and from every SQL string the code builds, and that `duckdb_secrets()` shows it redacted (P1) |
| R6 | §2.4 `parquet_source` binds the S3 glob (locator + `-u`); §2.3 bound `CREATE SECRET`; local SQL byte-identical (unit test) |
| R7 | §2.3 `open_s3_connection` through `load_extension`; amended error text; `offline_tests.rs` untouched; a test that a local query ignores a garbage `httpfs` file in `XDU_EXTENSION_DIR` (P1) |
| R8 | §2.7 refusal at `xdu-rm.rs:27` (P1) |
| R9 | §2.11 per-phase pages + README; CI literal counts |
| R10 | §2.10 in-process s3s-fs on loopback; CI fetch + `--include-ignored` + a passed count equal to the `--list` count guard on both legs; `#[ignore]` locally |

## 3. Invariant gate (AGENTS.md constitution check)

Walked before research (gate #1: sane; touches §2, §2b, §3, §5, §7, §10, §11, §13; no conflict that
blocks the intent) and again against this design (gate #2):

- **§1 schema** — untouched. `get_schema()` and `INDEX_FORMAT_VERSION` unchanged; S3 chunks are the
  same arrow-rs bytes.
- **§2 atomic finalize** — local finalize unchanged. On S3 a chunk is one PUT at its final key; a
  single PUT is all-or-nothing, so no reader ever sees a partial object. Per-partition atomicity
  stays absent on both transports (the known limitation). Bend recorded below (row 2).
- **§2b marker** — preserved step for step: every rejecting check (LIST pre-flight, work queue,
  reserved and S3-key names) precedes the marker DELETE; the PUT happens only on the success path;
  readers refuse before emitting rows; the cap holds over S3 via size-then-content (D8).
- **§2c fail-loud** — unchanged; an upload error fails the run like a write error does today.
- **§3 partitions** — no new reserved name; `__root__` and `.xdu-complete` map to keys unchanged;
  `index_glob` stays the local glob's one place, `index_glob_s3` the S3 one.
- **§4 xdu-rm** — only an early refusal; no destructive path changes.
- **§5 injection** — S3 locator and partition names bound; `CREATE SECRET` fully bound; local raw
  `format!` note stands (R6, GOAL Non-goal); the locator parser refuses glob metacharacters and
  userinfo. `AGENTS.md`/invariants §5 text gains the S3 binding rule in P2.
- **§6 Unix-only** — untouched.
- **§7 concurrency** — rayon pool and driver threads unchanged; `S3Sink`'s runtime threads are added
  to the stated thread budget in P3.
- **§8, §9, §12** — untouched (`xdu-view`'s sort mapping and terminal handling do not move).
- **§10 CLI** — no new flag; `-o`/`-i` widen to accept `s3://` with their pages in the same commit.
- **§11 altitude** — xdu-view queries move into the lib (improves).
- **§13 conventions** — tarball layout and pinned set unchanged; new runtime crates must pass the
  manylinux_2_28 glibc and NEEDED-library checks (ring, rustls, no OpenSSL); man-page counts
  re-derived; no spec ids in source.

### Deviation justifications

Rows 1, 4 and 5 were questions against R-ID wording; the foreman resolved all three by amending
the GOAL at the PLAN gate (`0f0edbd`), so each is now a pointer. Rows 2, 3 and 6–9 were accepted as
written.

| Deviation | Why needed | Simpler alternative rejected because |
|-----------|-----------|--------------------------------------|
| 1. Resolved by GOAL amendment `0f0edbd`: R5/R6 now require the bound `CREATE SECRET` after `LOAD`, redacted in `duckdb_secrets()`. No longer a deviation (evidence: digest D2) | — | — |
| 2. §2: S3 chunks have no `.partial` + rename; each is one PUT at its final key | An object store has no rename; copy-and-delete is neither atomic nor cheap | Emulating `.partial` with copy+delete: two requests and a window with both keys, for no atomicity gain over a single PUT |
| 3. The S3 prune deletes every higher-numbered chunk, past gaps; local stops at the first gap | One LIST already names every key; R1 asks for "no stale chunk" | Mirroring the gap stop: possible, but it keeps a stale chunk a foreign edit left behind, which R1 forbids. The gap only arises from such an edit |
| 4. Resolved by GOAL amendment `0f0edbd`: R1 now refuses, before the marker clear, a top-level name that is not UTF-8 or not safe as an S3 key segment and a DuckDB glob. No longer a deviation | — | — |
| 5. Resolved by GOAL amendment `0f0edbd`: R4 now fails an absent prefix for readers only; for `xdu` an absent or empty prefix starts a new index. No longer a deviation | — | — |
| 6. Both directions ignore `HTTP(S)_PROXY`/`NO_PROXY`; the pages say so | DuckDB 1.4.4 ignores them and reqwest honours them, so honouring them in the writer alone could split writer and readers onto different routes (D5) | Honouring env proxies in the writer only: incoherent. A shared proxy setting: new surface the pilot does not need. Follow-up `issues/s3-proxy-support.md` |
| 7. New runtime dependencies: `object_store` (ring), `reqwest`, `rustls`, `tokio`, `futures`, `bytes` (26 lockfile crates). Dev-only: `s3s`, `s3s-fs`, `hyper-util` | §2b needs DELETE and LIST, which httpfs lacks; R1 needs byte-identical chunks, which `COPY` cannot give (D3). R10 needs an endpoint with SigV4 and no container (D6) | Hand-rolled SigV4 over `ureq`: 15 crates and no runtime, but ~500 lines of signing and XML that a pilot would own. Kept as the fallback if ring/manylinux fails |
| 8. xdu-view's 4 queries move from the binary into `src/view_query.rs` | R2/R10 require testing xdu-view over S3, and the TUI never runs in CI | A hidden dump flag: new CLI surface in high-blast `cli.rs` plus man-page obligations. `#[cfg(test)]` in the bin cannot reach the binaries or the S3 fixture |
| 9. Wire tests are `#[ignore]` by default and CI runs them with `--include-ignored` | The pinned httpfs is not in the repo, and a developer without it must see "ignored", not `ok` (R10). CI's guard (passed = listed) keeps them from vanishing | A runtime `SKIP` print that returns early: libtest reports it `ok`, a silent pass by R10's standard |

## 4. Rabbit holes (resolved)

- **Credentials into DuckDB with autoload off** → `Config` route impossible; bound `CREATE SECRET`
  after `LOAD` works and redacts ([`00-digest.md`](research/00-digest.md) D2,
  [`01`](research/01-reader-path.md) §0). Adopted by GOAL amendment `0f0edbd`.
- **Writer transport** → object_store 0.14.2 on ring; DuckDB has no DELETE and re-encodes
  ([`03`](research/03-writer-transport.md)).
- **Loopback endpoint without containers** → s3s-fs in-process; MinIO archived, versitygw broken on
  macOS, moto cannot reject a bad secret faithfully ([`04`](research/04-loopback-endpoint.md)).
  This host's sandbox allows loopback bind and cross-process connect.
- **R3 measurement** → counting proxy, byte- and range-based, with positive and negative controls;
  DuckDB's counters disagree with the wire ([`04`](research/04-loopback-endpoint.md) §3).
- **Bounded marker read** → size via HEAD, then content; one statement cannot bound it
  ([`04`](research/04-loopback-endpoint.md) §2b).
- **Reader blast radius** → 15 statements, each one `?`, local SQL byte-identical
  ([`01`](research/01-reader-path.md) §2).
- **Archive reuse** → the write seam ports; staging and tool-specific S3 flags are dropped
  ([`02`](research/02-writer-path.md)).
- **httpfs in CI** → nothing fetches it for tests today; `fetch.sh` into `$RUNNER_TEMP`
  ([`05`](research/05-ci-docs-tests.md) §1).

## 5. Risks & open questions

**Resolved at the PLAN gate** (GOAL amendment `0f0edbd`, 2026-11-05):

- **Q1 → R5/R6:** credentials go only as the bound parameters of one prepared `CREATE SECRET`
  after `LOAD`; `duckdb_secrets()` shows the secret redacted. Built as §2.3 describes.
- **Q2 → R1:** an unsafe top-level name is refused before the marker clear, naming the directory.
- **Q3 → R4:** an absent prefix fails a reader; for `xdu` an absent or empty prefix starts a new
  index.

No open question remains for build.

**Risks:**

- **httpfs loaded from a debug build on linux_amd64** has never been observed (claim 13 is
  inference). P1's run B reads it from the ubuntu test leg. If it fails, P1 stops there.
- **ring under manylinux_2_28 inside `release.yaml`:** ring builds there today only as a build
  dependency; P3's run B reads the release legs. **Binding (foreman, PLAN gate):** P3's refusal of
  `openssl-sys`, `native-tls` and `aws-lc-sys` in the runtime tree stays a gate. If ring fails in a
  release leg, the fallback is hand-rolled SigV4 over `ureq`, and it returns to the foreman as a
  GOAL/PLAN question, never a silent swap. That question should weigh that `ureq`'s rustls also
  defaults to ring.
- **Error classification by DuckDB text** (D8) depends on httpfs's message strings. Mitigation: the
  glob-before-HEAD split carries the main distinction, and every class has a test, so a wording
  change in a future engine bump fails loudly.
- **Calendar:** the five phases total about 3 days, the top of the box. xdu-view (P4) is the
  designed cut. Any cut goes back through the GOAL gate as an amendment.
- **Follow-up seeds**, filed in P5 with `ROADMAP.md` entries:
  - `issues/s3-index-rm.md`;
  - `issues/s3-credential-chain.md`;
  - `issues/local-reader-bound-parameters.md`;
  - `issues/s3-proxy-support.md` (new, D5).
- **Side defect:** `README.md:116` uses `-b` where the flag is `-B`. Fix it opportunistically when
  editing the README.

## 6. Verification strategy

- **Unit tests:** locator parse (S3 vs local, non-UTF-8 stays local, refusals) and resolver
  precedence against fixture files (env beats profile, credentials file beats config, `services`
  endpoint, fail-closed cases, redacted `Debug`). Also: `parquet_source` local strings
  byte-identical, `classify_s3_error` per measured text, and S3-key name validation.
- **Integration, always run:**
  - `xdu-rm -i s3://…` refuses with the message and exit non-zero (also through `XDU_INDEX`);
  - `xdu-find -i s3://…` with an empty `XDU_EXTENSION_DIR` names the path and both remedies;
  - a local query with a garbage `httpfs.duckdb_extension` in `XDU_EXTENSION_DIR` still answers
    (proves no load);
  - `offline_tests` unchanged.
- **Wire tests** (`tests/s3_tests.rs`, `--include-ignored`):
  - parity of `xdu-find` local vs S3 across filters, `--count`, `--top`, sort and `-u`, with output
    compared byte for byte;
  - R3 via the proxy;
  - R4 matrix;
  - the marker cap: a 2 MiB marker is refused, and the proxy saw no GET of it;
  - credentials from env and from `AWS_PROFILE` + fixture files, with the secret absent from
    stdout and stderr;
  - `xdu -o s3://` round trip: the endpoint root equals a local run's tree (key set, the sorted
    rows per partition read back through `xdu-find`, the marker minus `completed_at`);
  - a re-run with fewer files leaves no stale chunk;
  - a read-only-credential run leaves the old marker in place;
  - an injected mid-run failure writes no marker;
  - `TMPDIR` and `HOME` stay empty;
  - xdu-view's lib queries local vs S3 equal across all 7 `SortMode`s.
- **CLI drive in a throwaway index:**
  `.agents/factory/bin/temp_index.sh sh -c 'xdu-find -i "$XDU_INDEX" --count'` before and after,
  to prove the local path is unchanged.
- **Release gates:** `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings`,
  and the man-page render plus literal counts (CI on push; locally after installing `scdoc`).

---

*Backing research: [`research/00-digest.md`](research/00-digest.md) and briefs `01`–`05`.*
