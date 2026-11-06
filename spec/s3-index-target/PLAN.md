# PLAN — Build an index into S3 and query it in place

> **Status:** P1–P5 passed the PLAN gate, reconciled with GOAL amendment `0f0edbd`. P6 (§7,
> R11, GOAL amendment `86bf298`) awaits the PLAN gate · **Last updated:** 2026-11-05
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
| R11 | §7 (P6): one trust source from the resolver; the reader pins httplib, turns verification on and binds `ca_cert_file`; the writer trusts only that file's certificates; `tls_` wire tests behind a TLS front |

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

## 7. Phase P6 — TLS server authentication (R11)

Review cycle 3 finding 1: the readers accept any server certificate, and the writer verifies
against a different store. GOAL amendment `86bf298` adds R11. The probes behind every choice
below are in [`research/07`](research/07-tls-server-auth.md); the decision is digest D11.

### 7.1 Design

- **What the stacks do today (probed).**
  - **Reader.** The pinned httpfs reads through httplib, with OpenSSL 3.0.8 linked in and curl
    beside it (`httpfs_client_implementation = default` means httplib). It verifies nothing
    until `enable_server_cert_verification` is on. With it on and no `ca_cert_file`, httplib
    trusts OpenSSL's defaults: `SSL_CERT_FILE`, else the compiled-in `/etc/ssl`, which is wrong
    on RHEL-family hosts. It never reads the macOS keychain.
  - **Writer.** The writer's `reqwest` 0.13 trusts `rustls-platform-verifier`: the keychain on
    macOS, and on Linux the native store, which honours `SSL_CERT_FILE`.
  - **Neither honours `AWS_CA_BUNDLE`.** Both readers' settings are global `SET`s, and
    `SET ca_cert_file = ?` takes a bound value.
- **Trust source (`src/s3.rs`, the resolver).** `S3Settings` gains `trust: Option<TrustSource>`,
  resolved in `S3Settings::resolve` and `Some` exactly when the endpoint is `https`.
  - **`TrustSource`** holds the file's absolute path (valid UTF-8, since it is bound as text),
    its origin (`AWS_CA_BUNDLE` or the platform), and the DER certificates read from it.
  - **Resolution.** `AWS_CA_BUNDLE` when set and non-empty; set-but-empty counts as unset, as
    every variable the resolver reads does. Else the first file present in
    `PLATFORM_CA_BUNDLES`, in httpfs's own probe order:
    `/etc/ssl/certs/ca-certificates.crt`,
    `/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem`, `/etc/pki/tls/certs/ca-bundle.crt`,
    `/etc/ssl/ca-bundle.pem`, `/etc/ssl/cert.pem`.
  - **Reads.** The file is read through `EnvSource::read_file`, so unit tests feed fixtures for
    both origins. A file that is present but unreadable is an error; the resolver does not skip
    on to the next candidate.
  - **Certificates kept.** Every `CERTIFICATE` block is parsed
    (`rustls::pki_types::pem::PemObject`). Only certificates `rustls::RootCertStore::add`
    accepts are kept, since one that webpki cannot parse would otherwise fail the writer's
    client build. Keeping none is an error.
  - **Refused outright,** so that OpenSSL never loads what the writer leaves out:
    - a block that is not valid PEM, or a `CERTIFICATE` block that is not valid DER, since
      OpenSSL would fail on it at request time;
    - a `TRUSTED CERTIFICATE` block, which OpenSSL trusts and webpki cannot read.
  - **Dropped, and only these:** DER-valid certificates webpki rejects.
  - **Errors** come before any request and name `AWS_CA_BUNDLE` and the path, never a
    credential:
    - `AWS_CA_BUNDLE names <path>, which cannot be read (<kind>)`;
    - `… holds no usable CA certificate`;
    - `no platform CA bundle found (looked for <list>); set AWS_CA_BUNDLE to a CA file`;
    - a platform file that is present but unusable gets the same shape.
  - **One phrase belongs to the endpoint refusal alone:** "TLS certificate is not trusted". No
    resolver error may contain it, so a test can tell "refused the server" from "found no
    roots".
  - **Never a source:** `SSL_CERT_FILE`, `SSL_CERT_DIR`, OpenSSL's `/etc/ssl` default and the
    keychain. A plain `http://` override resolves no source, so a broken `AWS_CA_BUNDLE` does not
    stop it (R11: TLS does not apply).
- **Reader (`src/engine.rs`, `open_s3_connection_from`).** After the `LOAD`, before the
  `CREATE SECRET` and any request, one fixed batch runs:
  `SET httpfs_client_implementation = 'httplib'; SET enable_server_cert_verification = true;
  SET enable_curl_server_cert_verification = true`.
  - **Then the file.** `SET ca_cert_file = ?`, prepared, with `TrustSource`'s path bound, when
    there is a source.
  - **Where the text lives.** It comes from `S3Settings::duckdb_tls_statements()`, beside
    `duckdb_secret_statement`, so a unit test pins it without httpfs. Every error propagates.
  - **Why pin httplib:** curl ignores `ca_cert_file` (probed), so an engine whose `default`
    moved to curl would change the trust source silently. A removed `httplib` fails the `SET`
    loudly.
  - **Why the curl flag too:** it keeps that backend verifying, failing closed, if anything ever
    selects it.
- **Writer (`src/s3.rs`, `DirectConnector`).** The connector carries the source's DER
  certificates. Its client is built with
  `reqwest::ClientBuilder::tls_certs_only(certificates)`, so the platform verifier, the native
  store and `SSL_CERT_FILE` drop out. It never uses `tls_certs_merge` or
  `add_root_certificate`.
  - **Plain `http://` endpoint:** no source, so the list is empty and the client cannot complete
    any TLS handshake, a redirect included.
  - **Dependencies:** the writer gains no runtime crate. `rustls::pki_types` and
    `rustls::RootCertStore` are already in the tree.
- **One source, both directions.** The resolver reads the file once. The reader's OpenSSL
  reopens the same path at its first request; the writer uses the certificates read. The only
  gap is a file replaced between those two reads, which is a change of trust configuration by
  whoever controls the file, not a way around it.
- **Refusal wording (`src/lib.rs`, `src/s3.rs`).**
  - **Reader.** `classify_s3_text` gains `S3Cause::Untrusted` for httplib's "SSL server
    verification failed" (and curl's "SSL peer certificate", defensively).
  - **Writer.** `S3Sink::request_error` matches rustls's "invalid peer certificate" in the
    error chain, ahead of its unreachable branch, which today words a TLS refusal as "cannot
    reach".
  - **Shared message:** `the S3 endpoint's TLS certificate is not trusted while <what> for
    <index>; it must chain to a CA in the file AWS_CA_BUNDLE names, else the platform's CA
    bundle`. The marker-ordering guarantees are untouched. The writer's refusal comes at the
    pre-flight LIST, before the marker DELETE, so an existing index keeps its marker (R4's
    terms).
- **Tests (`tests/common/s3.rs`, `tests/s3_tests.rs`).**
  - **The TLS front.** `TlsFront`, on `TestS3`'s runtime, accepts on `127.0.0.1:0`, terminates
    TLS with `tokio-rustls` and relays plaintext to the counting proxy. Every existing proxy
    assertion still holds: ranged reads only, one recorded request per connection, nothing
    bypassing the proxy. For each connection it records the handshake's outcome, any alert the
    client sent, and whether application bytes followed.
  - **Why tests never count handshakes.** httplib finishes the handshake and only then checks
    the result (its refusal is "SSL server verification failed", not a connection failure). So a
    refusing reader shows a *completed* handshake, then a close with no application bytes. The
    rustls writer fails inside the handshake, and its retries can repeat the attempt.
  - **The rules that follow:**
    - **Refusal:** the front accepted at least one connection for the run, and the proxy
      recorded 0 requests.
    - **Writer refusal:** also at least one failed handshake that carries a certificate alert.
    - **Acceptance:** at least one proxied request per tool.
  - **Certificates.** `TestPki` generates with `rcgen`: a test CA, an unrelated CA, a leaf the
    test CA signs for `IP:127.0.0.1`, a test-CA leaf for `DNS:wrong.example` only, and a
    self-signed **end-entity** certificate for `IP:127.0.0.1`. It must not be a CA certificate:
    a self-signed CA certificate is refused as `CaUsedAsEndEntity`, which proves a different
    rule. Each CA's PEM goes in a temp directory outside the `Hermetic` directories.
  - **Hermetic children.** `Hermetic::command_for` also strips `SSL_CERT_FILE` and
    `SSL_CERT_DIR`. `AWS_CA_BUNDLE` already goes with `AWS_*`.
  - **New wire tests** (`tls_` prefix, `#[ignore]` like the rest; 46 → **51**). Each goes
    through the front and the proxy:
    1. `tls_a_self_signed_endpoint_is_refused_by_all_three_tools`.
       - **Setup:** a seeded index, the self-signed certificate, then the wrong-name test-CA
         leaf, each with `AWS_CA_BUNDLE` naming the test CA. The self-signed one is also tried
         with no `AWS_CA_BUNDLE` (platform bundle).
       - **Asserts:** `xdu` (into the seeded prefix), `xdu-find --count` and `xdu-view` each
         exit non-zero with empty stdout. Stderr carries the exact phrase "TLS certificate is
         not trusted" and `AWS_CA_BUNDLE`. The refusal rule holds: at least one front
         connection per run and 0 proxied requests, plus, for `xdu`, a failed handshake with a
         certificate alert. The prefix's objects are byte-identical afterwards, marker
         included, and `HOME`, `TMPDIR` and cwd are empty.
       - **Platform leg.** The run with no `AWS_CA_BUNDLE` panics, naming the list, if no
         platform bundle exists on the host. It never passes on a "no roots" refusal.
    2. `tls_an_endpoint_the_named_ca_signed_answers_as_plain_http_does`.
       - **Writer:** `xdu` over TLS into one prefix and over HTTP into another, from one source
         tree. The objects are equal apart from the marker's `completed_at`.
       - **`xdu-find`:** `--count`, `-u`, `--top 3` and the default listing give byte-identical
         stdout over both.
       - **`xdu-view`:** the `view_query` statements give equal results on an in-test
         `open_s3_connection_from` connection for each. The binary, driven with no terminal,
         gets past the marker read through the front (recorded requests, no certificate
         cause) before it stops for want of a terminal.
       - **Every run:** no whole-object GET, and at least one proxied request per tool.
    3. `tls_an_unusable_ca_bundle_is_refused_before_any_connection`.
       - **Setup:** `AWS_CA_BUNDLE` names a missing path, a directory, an empty file, and a PEM
         holding only a key.
       - **Asserts:** the three tools exit non-zero naming `AWS_CA_BUNDLE`, and the front
         accepted 0 connections. The same four values over the plain-HTTP proxy still answer
         `--count`.
    4. `tls_only_the_resolved_file_is_trusted`.
       - **Setup:** the endpoint shows the test-CA leaf. In (a), `AWS_CA_BUNDLE` is unset and
         `SSL_CERT_FILE` names the test CA. In (b), `AWS_CA_BUNDLE` names the unrelated CA and
         `SSL_CERT_FILE` names the test CA. `SSL_CERT_DIR` is not used: OpenSSL reads it only
         as a hashed directory, so naming a file would prove nothing.
       - **Asserts:** all three tools refuse under the refusal rule, with stderr carrying "TLS
         certificate is not trusted", which rules out a "no roots" refusal.
       - **What it proves:** case (a) fails on today's readers on every host. It fails on
         today's writer only on Linux, since macOS's platform verifier ignores
         `SSL_CERT_FILE`. So the writer half of red-first shows on the Ubuntu leg, not on a Mac.
    5. `tls_the_reader_connection_pins_verification_and_the_ca_file`. After
       `open_s3_connection_from`, `current_setting` reads `httplib`, `true`, `true` and the
       resolved absolute path for an `https` endpoint, and an empty `ca_cert_file` for an
       `http` one. One run puts the bundle under a directory whose name holds a `'`; the path
       reads back intact, which shows it is bound, not SQL text.
  - **Unit tests (`s3::tests`, always run).**
    - `the_trust_source_is_the_file_aws_ca_bundle_names`;
    - `an_empty_aws_ca_bundle_is_unset`;
    - `without_aws_ca_bundle_the_first_platform_bundle_present_is_the_source`;
    - `no_platform_bundle_is_refused_naming_aws_ca_bundle`;
    - `an_unusable_bundle_is_refused_naming_aws_ca_bundle` (malformed PEM, invalid DER, a
      `TRUSTED CERTIFICATE` block);
    - `no_resolver_error_says_the_certificate_is_not_trusted`;
    - `a_certificate_webpki_rejects_is_dropped_and_the_rest_kept`;
    - `a_plain_http_endpoint_resolves_no_trust_source`;
    - `the_tls_statements_are_fixed_text_and_bind_only_the_path`.
  - **Red first.** Wire test 1 must fail on `d5913be`, with the readers answering, and wire test
    4(a) must fail there too. Record both in the commit body.
  - **Dev-dependencies, pinned exactly:** `rcgen = "=0.14.10"` (default features: `crypto`,
    `pem`, `ring`; no aws-lc) and
    `tokio-rustls = { version = "=0.26.4", default-features = false, features = ["ring"] }`.
- **Real roots on the release platforms: a CI probe.** New
  `packaging/extensions/aws-tls-probe.sh BINDIR [EXTDIR]`. Nothing local runs it: it is not
  part of `cargo test` or `tests/s3-wire.sh`.
  - **What it runs.** Under `env -i` with a cold `HOME`, it runs `xdu-find --count`, `xdu-view`
    and `xdu SRC -o` against `s3://xdu-tls-probe/p` on AWS (`us-east-1`, virtual-hosted, no
    override). It uses AWS's documented example key pair, `AKIAIOSFODNN7EXAMPLE`, so no account
    and no secret.
  - **Asserts, per tool:** exit non-zero, and stderr carries an HTTP answer. That is any status:
    `HTTP [0-9]{3}`, "refused the credentials", or "does not exist at the S3 endpoint". A
    squatted bucket in another region may answer 301 or 400, and that still proves the
    handshake verified. Stderr must carry neither "certificate" nor `AWS_CA_BUNDLE`. "cannot
    reach" is retried twice, then fails.
  - **What it prints:** the matched stderr line for each tool, so a reader of the log sees the
    real answer, and the first platform bundle present, for information only.
  - **Where it runs:**
    - in `test.yaml`'s test legs, after the S3 step: Debian-family `ubuntu-24.04` and
      `macos-15`;
    - from `check.sh` on every release leg: manylinux_2_28 containers (RHEL-family, `/etc/pki`)
      on amd64 and arm64, and macOS. It runs while the extracted `lib/` is still in place:
      before the `expect_absent` block (which moves it aside) and before the cold-`HOME` check.
      Placed after them, a correct reader would die naming the missing extension.
    - **Release triggers.** P6's diff touches `src/engine.rs` and `Cargo.toml`, so the release
      workflow runs on its pull request. `release.yaml`'s `pull_request.paths` gains
      `src/s3.rs` and `src/lib.rs`, so that a later change to the trust list re-runs the
      RHEL-family probe before a tag rather than at one.
  - **What it proves:** the resolver finds a usable bundle on each of those hosts, each platform
    build of httpfs accepts it, and AWS's real chain verifies against it in both TLS stacks.
  - **What it cannot prove:** that verification is on. With it off, AWS answers 403 too. The
    loopback tests own that.
  - **Cost:** a release now depends on AWS answering, as it already depends on the DuckDB
    extension host. See Q4.
- **Docs and manual.**
  - **Man pages.** `doc/xdu.1.scd`, `doc/xdu-find.1.scd` and `doc/xdu-view.1.scd` get
    `AWS_CA_BUNDLE` in `ENVIRONMENT`, plus a sentence in the S3 section: every `https`
    connection verifies the server against that file, else the first platform bundle (the
    list); `SSL_CERT_FILE` is not read; nothing turns verification off. Counts are re-derived;
    `AWS_CA_BUNDLE` joins each page's `check` list in `test.yaml` and `AGENTS.md`'s
    duplicated-literal sentence in the same commit (§13).
  - **`README.md`:** one sentence in `### Indexes in S3`.
  - **`AGENTS.md`:** invariant 5 mirrors the new bullet below, and the High-risk entries for
    `src/engine.rs` and `src/s3.rs` name the trust source. Testing names the `tls_` prefix and
    the front, and Commands and the workflow carry 51.
- **Invariant text** (a new bullet in `invariants.md` §5; §5 is in §1–§12, so a breach is
  auto-CRITICAL). The build applies it verbatim:

  > - **Every S3 connection authenticates the server, against one trust source.**
  >   `S3Settings::resolve` picks the source once for both directions: the file `AWS_CA_BUNDLE`
  >   names, else the first of `s3::PLATFORM_CA_BUNDLES` present. A missing, unreadable or
  >   certificate-free source refuses before any request, naming `AWS_CA_BUNDLE`.
  >   - **Writer.** `S3Sink`'s client trusts only that file's certificates
  >     (`tls_certs_only`): never the platform verifier, built-in roots or
  >     `tls_certs_merge`.
  >   - **Reader.** `engine::open_s3_connection` runs fixed text after the `LOAD`: it pins
  >     `httpfs_client_implementation` to `httplib`, turns `enable_server_cert_verification` and
  >     `enable_curl_server_cert_verification` on, and binds the path as `SET ca_cert_file = ?`.
  >     The path is never SQL text.
  >   - **Nothing disables verification:** no `danger_accept_invalid_*`, `tls_danger_*`,
  >     `allow_invalid_certificates` or custom verifier; no `false` for either flag; no option
  >     or variable that turns it off.
  >   - **Nothing adds roots beside the one source:** no `tls_certs_merge`,
  >     `add_root_certificate`, `with_root_certificate`, `webpki-roots`, `rustls-native-certs`
  >     or platform verifier.
  >   - **Never trust sources:** `SSL_CERT_FILE`, `SSL_CERT_DIR`, OpenSSL's compiled-in
  >     directory and the macOS keychain. A plain `http://` endpoint override resolves no source.
  >   - **The tests that hold it:** the `tls_` wire tests. A self-signed, unknown-CA or
  >     wrong-name endpoint is refused by all three tools; an endpoint the named CA signed
  >     answers as plain HTTP does; an unusable `AWS_CA_BUNDLE` stops each tool before it
  >     connects.

### 7.2 Requirement → design map (P6)

| R-ID | Design element(s) |
|------|-------------------|
| R11 | §7.1 `TrustSource` in `S3Settings::resolve` (one source, both directions; refusal naming `AWS_CA_BUNDLE`); reader's fixed `SET`s and bound `ca_cert_file`; writer's `tls_certs_only`; untrusted-certificate cause in both directions; wire tests 1–5; unit tests; man pages; invariant §5 bullet |
| R4 | The certificate refusal follows R4's terms: no rows, and for `xdu` a refusal at the pre-flight LIST, before the marker DELETE (wire test 1 checks the seeded marker survives) |
| R5 | The resolver stays the one resolver; `AWS_CA_BUNDLE` joins the variables it reads |
| R9 | `AWS_CA_BUNDLE` on the three pages and in the README, under the literal-count gate |
| R10 | The `tls_` tests run on the loopback endpoint in CI's S3 step; the minimum rises to 51 |

### 7.3 Invariant gate (P6)

- **Gate #1 (before the design): sane.** It touches §2b, §5, §10, §11 and §13. Nothing in R11
  is at odds with them. Verification is on the same footing as `CREATE SECRET`: fixed text plus
  bound values.
- **Gate #2 (against the design):**
  - **§2b.** Untouched: every refusal precedes the marker DELETE. A bad trust source fails at
    resolve; a bad certificate fails at the pre-flight LIST.
  - **§5.** The new statements are fixed text, and the one path is a bound value. The rule
    joins §5 as the bullet above.
  - **§7.** No new thread. The front runs on the test runtime.
  - **§10.** No new flag. A new variable is documented on its pages in the same commit.
  - **§11.** Trust selection is a library function, testable without the binaries.
  - **§13.**
    - No runtime crate is added, and the P3 dependency floor holds for dev-dependencies too
      (the gate checks `-e normal,dev`).
    - The new literal follows the three-place rule, and no spec ids go in source.
- **Deviations:** one, below. Empty is not reachable, since R10 needs a TLS endpoint and none
  exists in the tree.

| Deviation | Why needed | Simpler alternative rejected because |
|-----------|-----------|--------------------------------------|
| 10. Dev-dependencies `rcgen` (new, with `yasna` and `pem`) and `tokio-rustls` (already locked) | R11's tests need a TLS endpoint on loopback with certificates made per run | Committed PEM fixtures: private keys in a public repo trip secret scanners, and fixed expiry dates rot. A std-only TLS relay over `rustls::StreamOwned` cannot split one stream across two relay threads without a lock |

- **Adversarial review of this design** (xdu-plan Step 3: the tests and the CI probe are
  gate-shaped). One read-only reviewer raised eight points before this section was committed,
  and all are adopted above:
  1. httplib verifies after the handshake, so tests assert on proxied requests, not handshakes.
  2. The refusal phrase is unique, so a "no roots" refusal cannot pass a certificate test.
  3. The static greps became a ban list and an allowlist, and a `'` path proves binding.
  4. `SSL_CERT_DIR` was dropped, and the Linux-only writer half of the red-first is recorded.
  5. The probe moved ahead of `check.sh`'s `lib/` move, and `release.yaml`'s paths were
     widened.
  6. The probe accepts any HTTP status.
  7. The PEM rules make both stacks load the same set.
  8. Names are checked in the list file.

### 7.4 Risks and open questions (P6)

- **Q4 (for the foreman at the PLAN gate): the CI request to AWS against GOAL's Non-goals.**
  - **The tension.** GOAL rules out "a real-provider check as an acceptance gate, whether manual
    or a CI job against a cloud account". The probe uses no account, and R11's acceptance stays
    on loopback. But it is a CI step that fails the build on an AWS answer.
  - **Recommendation:** keep it. It is the only check of the platform-roots half of R11 on the
    release platforms.
  - **If the ruling is no:** drop the `aws-tls-probe.sh` checklist item and its three gate
    predicates. The rest of P6 stands.
- **The writer's trust narrows on macOS.** Today it trusts the keychain, including roots an
  administrator or MDM added. After P6 it trusts `/etc/ssl/cert.pem`, which carries Apple's
  bundle and not those additions. A site whose endpoint chains to such a root sets
  `AWS_CA_BUNDLE`, and the pages say so. The readers never trusted the keychain.
- **A platform bundle the list does not name:** for example `/etc/ssl/certs/` as hashed files
  only. It refuses, naming `AWS_CA_BUNDLE`, which is the designed failure. The release-leg probe
  covers the Debian, RHEL and macOS families.
- **Real AWS answers:** a nonexistent bucket may answer 404 rather than 403. Both count. A
  regional redirect would land in the writer's generic branch and fail the probe, so the build
  confirms the answers on the first CI run (run B), not here.
- **Size:** about 0.75–1 day of build, over the half-day aim.
  - **The core** (resolver, both stacks, wording, front, five wire tests, docs and invariant):
    about 0.6.
  - **The CI probe in two workflows:** about 0.25. It is the separable part if the foreman wants
    P6 smaller (Q4).

### 7.5 Verification (P6)

- **Wire:** `tests/s3-wire.sh tls_ 5` with the five names checked in its log. Then the whole file
  at its listed count, which must be at least 51 and equal the minimum in `test.yaml` and in
  `AGENTS.md`'s command.
- **Unit and suite:** the nine `s3::tests` names, then `cargo test --locked`.
- **Static predicates.** Each fails on today's tree where it should:
  - **Ban list** (case-insensitive, over `src/`):
    - anything that disables verification: `danger_accept_invalid`, `tls_danger_`,
      `allow_invalid_certificates`, `dangerous()`, `with_custom_certificate_verifier`, and
      either flag set to `false`, or to `0` in any spelling;
    - anything that adds roots beside the source: `tls_certs_merge`, `add_root_certificate`,
      `with_root_certificate`, `tls_backend_preconfigured`, `use_preconfigured_tls`, and any
      use of the `webpki_roots`, `rustls_native_certs` or `rustls_platform_verifier` crates.
  - **Allowlist:** every non-comment `src/` line that names `cert_verification`, `ca_cert_file`
    or `httpfs_client_implementation`, outside a `#[cfg(test)]` module, matches the expected
    text exactly. A comment cannot satisfy it, and a stray `SET` cannot hide in it.
    - **Constraints this puts on the build:** each fixed statement sits whole on one source
      line. Production code names those settings nowhere else; doc comments are exempt.
    - **The ban list scans test modules too,** so no test may spell a banned string, even
      inside an assertion.
  - **What must be there:** `tls_certs_only` called in `src/s3.rs` outside its test module.
  - **Behaviour behind the text:** wire test 5 reads the settings back, and its `'` path shows
    the path is bound.
  - **Errors not dropped:** no `let _ =` around a TLS statement.
- **Docs and manual:** `AWS_CA_BUNDLE` is on three pages and the README, and in each page's
  `check` block (block-scoped, so a comment cannot satisfy it). `invariants.md` and `AGENTS.md`
  carry the rule.
- **CI probe and dependencies:**
  - the probe script parses (`sh -n`);
  - its run line, not a comment, is in `test.yaml` and `check.sh`, and in `check.sh` it comes
    before the line that moves `lib/` aside;
  - `release.yaml`'s paths name `src/s3.rs` and `src/lib.rs`.

  The dependency floor runs over `normal,dev`. Then clippy and fmt.
- **Selection by the list file.** The wire-test names are checked against
  `target/wire-tls_.list`, not the log, because libtest interleaves stderr into the log.
  `s3-wire.sh` already proves passed equals listed.
- **Run B evidence** (CI only):
  - both test legs' `s3-wire: '' 51 of 51 passed` and their `aws-tls-probe` lines;
  - each release leg's `aws-tls-probe` lines before `extension-check: ok`;
  - the Packaging literal step's `OK:` line.

### 7.6 Review cycle 4 remediation (P6 and P5 reopened)

Review cycle 4 found two more splits between the two directions' TLS checks. The human ruled
option 1 (GOAL Clarification of 2026-11-06): finding 1 becomes R11's one recorded exception,
with its root fix seeded as a follow-up **fix** cycle, and finding 2 is fixed here.

- **Finding 2: a trust source must hold a root (`src/s3.rs`, `bundle_certificates`).**
  - **Why the stacks differ.** webpki takes every certificate in the file as an anchor, so an
    intermediate alone anchors the writer. OpenSSL, without `X509_V_FLAG_PARTIAL_CHAIN`, which
    httpfs does not set, accepts a chain only when it ends at a self-signed certificate in its
    store. So a file with no self-signed root can only ever split the two directions.
  - **The rule.** After the existing PEM rules, the kept certificates must include at least one
    root. Otherwise the file is refused before any request, in the resolver's own words:
    `AWS_CA_BUNDLE names <path>, which holds no root CA certificate (one issued and signed by
    itself); …`, or for a platform bundle the same reason with `set AWS_CA_BUNDLE to a CA file`.
    The words never say "not trusted".
  - **What counts as a root.** Both of these must hold:
    1. The certificate's issuer `Name` is byte-identical to its subject `Name`.
    2. Its signature verifies under its own `subjectPublicKeyInfo`, with one of the algorithms
       the writer's TLS stack verifies: `rustls::crypto::ring::default_provider()`'s
       `signature_verification_algorithms.all`, matched on both algorithm identifiers as
       webpki matches them.
  - **Parsing.** The fields come from the same DER walk `is_certificate_der` does
    (`der_element`): the signed `tbsCertificate` bytes, the outer signature algorithm and value,
    and the TBS's issuer, subject and key. No crate is added.
  - **Why this is sound.**
    - Both conditions are what a self-signed root *is*. An intermediate fails (1).
    - A forged "self-issued" certificate, its issuer set to its subject but signed by another
      key, fails (2).
    - OpenSSL marks a certificate self-signed by a canonical name match, an authority key
      identifier that, if present, names its own key, and a signature algorithm of its key's
      family. It does not check the self-signature. A certificate that passes both conditions
      passes that test too, unless its authority key identifier names another key, which no CA
      issues for a root. So a file this accepts holds an anchor OpenSSL can end a chain at.
  - **What it costs.**
    - A file whose only roots are self-signed with an algorithm ring does not verify, such as
      SHA-1 RSA, is refused, though OpenSSL would anchor there (it does not check a root's
      self-signature).
    - Byte equality is stricter than OpenSSL's canonical name compare.
    - Both fail closed, naming the file. Platform bundles carry many SHA-256 and ECDSA roots, and
      the build checks `/etc/ssl/cert.pem` and a Mozilla-derived bundle locally. The CI probe
      shows the Debian, RHEL and macOS bundles on CI hosts.
  - **What it does not close.** A file holding a root plus an unrelated intermediate whose own
    root is absent still lets the writer anchor at that intermediate. That is the same class at
    a narrower edge. It is recorded in the root-fix seed, which owns making the two directions
    agree on the chain, not only the name.
- **Finding 1: the recorded exception.** No code change. httplib's CN fallback is inside the
  pinned extension, and the non-goals bar changing the pinned set.
  - **`invariants.md` §5** names the exception in the style of its local-path exception and
    cites the seed. `AGENTS.md` invariant 5 mirrors it.
  - **A wire test documents it.** A test-CA leaf whose CN is `127.0.0.1` and whose SAN is
    `DNS:wrong.example` (and a second one with no SAN) is refused by `xdu` under the refusal
    rule, and answered by `xdu-find` and `xdu-view` today. The test asserts today's reader
    behaviour, and a comment names the seed, so the root fix must flip it. That makes it the
    seed's red-first.
- **Seeds.** `issues/s3-reader-tls-hostname.md` (kind `fix`) carries the review's reproduction,
  the exception test as its red-first, and two candidate routes: an in-process loopback relay
  that terminates TLS with the writer's rustls client and trust source and serves httpfs plain
  HTTP through its `http_proxy` setting, and an upstream httpfs or httplib change. Removing the
  R11 and §5 exception is its acceptance. `issues/s3-dotted-bucket-names.md` carries finding 3,
  with path style as a candidate. Both join P5's enforced seed list and get `ROADMAP.md` entries.
- **Tests.** Red first:
  - **Unit:** a bundle with no root is refused naming `AWS_CA_BUNDLE`, for both origins; the
    root predicate itself; and every platform bundle present on the host holds a root.
  - **Wire:** an intermediate-only bundle is refused by all three tools before any connection,
    and a root-plus-intermediate bundle accepts a leaf the intermediate signed; and the
    exception test.
  - **The listed count** goes from 51 to 53 in the workflow, `AGENTS.md` and the P5 and P6 gates.
- **Docs.**
  - **Man pages and README.** `xdu.1`, `xdu-find.1`, `xdu-view.1` and the README say that a
    private CA's certificate for the endpoint must name the host in its SAN, that the file must
    include the root, and that the readers' CN fallback is a known limitation.
  - **No counted literal changes.** `AWS_CA_BUNDLE` stays at two per page.

---

*Backing research: [`research/00-digest.md`](research/00-digest.md) and briefs `01`–`07`.*
