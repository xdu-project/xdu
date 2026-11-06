# Digest — consolidated decisions for s3-index-target

**Evidence base, adopted rather than re-derived:**
[`research/duckdb-native-s3-reads.md`](../../../research/duckdb-native-s3-reads.md) (claims 1–13,
the cost model) and its probes under `bench/spikes/`. Briefs `01`–`05` below answer what that note
left open. Two of its claims did not survive this round: claim 11 holds only with autoload on (D2),
and its "readers need no S3 client" stands, while its MinIO instrument does not (D6). One
recommendation per question follows, so `PLAN.md` does not re-argue them.

## D1 — Readers query in place through httpfs; nothing is staged

Every chunk request is a ranged GET. Measured again this round on three endpoints (`04`): a
22.1 MiB index in 8 chunks over 4 partitions answered `count(*)` with 1 LIST plus 8 ranged GETs of
16 KiB, 0.57–0.58% of stored bytes, and no query issued an unranged GET. The archive's staging
(`archive/s3-index-target-p2`, D1/D3 of its digest) is dropped whole: `materialize_index`,
`read_chunk` and the temp directory do not come back.

## D2 — Credentials reach DuckDB as bound parameters after `LOAD`, not through `Config`

Probed here against DuckDB 1.4.4 (the crate's bundled engine version) with the official
`osx_arm64` httpfs:

- `Config` with any `s3_*` key and autoload off fails **at open**: `Invalid Input Error: The
  following options were not recognized: s3_region`. With autoload on it fails for want of a
  versioned extension tree: `To set the s3_region setting, the httpfs extension needs to be loaded.
  But it could not be autoloaded.` Source reading agrees (`01` §0: `LoadExtensionSettings` runs at
  open and needs autoload). Spike claim 11 passed only because autoload was on and `$HOME` held an
  installed httpfs.
- After `LOAD '<path>'`, all of these succeed with the secret only in a bound parameter:
  `CREATE SECRET s (TYPE s3, KEY_ID ?, SECRET ?)`; a prepared `SET s3_access_key_id = ?`;
  `SET VARIABLE k = ?` followed by `SET GLOBAL … = getvariable('k')`. `duckdb_secrets()` prints
  `secret=redacted`; `duckdb_settings()` prints a `SET` value in clear.

**Decision:** one prepared `CREATE SECRET` (temporary, in-memory, `TYPE s3`, `PROVIDER config`)
with every field bound, issued by `engine::open_s3_connection` immediately after loading httpfs.
It also overrides what httpfs reads from `AWS_*`/`DUCKDB_S3_*` at load (`03` risk 1), because a
matching secret wins over settings. This keeps R5/R6's purpose (no secret in SQL text) but not
their letter ("through DuckDB's `Config` API"). **Amended at the PLAN gate** (GOAL `0f0edbd`): R5
and R6 now name the bound `CREATE SECRET` after `LOAD`, with the secret redacted in DuckDB's
secret listing.

## D3 — The writer uploads with a Rust client: object_store 0.14.2 on ring

DuckDB cannot be the writer (`03` §A): `COPY` re-encodes every chunk through DuckDB's own Parquet
writer, so R1's "same index" fails, and httpfs has no SQL path to DELETE (`OVERWRITE is not
supported for remote file systems`), so §2b's marker clear and the stale-chunk prune are
impossible. object_store 0.14.2 with `aws-base,reqwest,ring` plus `reqwest` 0.13
`rustls-no-provider` and `rustls` 0.23 `ring`: 26 new lockfile crates, MSRV 1.85, `cargo check`
green. ring already builds in the manylinux_2_28 release legs as a `libduckdb-sys` build
dependency; aws-lc-sys (the archive's `aws` feature) has never been built there, and any OpenSSL
option fails the release job's dynamic-library allow-list. Install the ring provider once before
any client (`panic = "abort"` turns reqwest's missing-provider panic into a dead process).
Fallback: hand-rolled SigV4 over `ureq` (15 crates, no runtime, ~500 lines). aws-sdk-s3 (76 crates,
two crypto stacks) and rust-s3 (aws-lc, no MSRV) lose on every axis.

Builder: `AmazonS3Builder::new()`, never `from_env()` (it reads more `AWS_*` than R5 allows, ignores
the shared files, and falls through to instance metadata); static keys always; a custom
`HttpConnector` around `reqwest::Client::builder().no_proxy()`; a bounded `RetryConfig` and
timeouts so an unreachable endpoint fails in seconds (R4).

## D4 — One resolver in `src/s3.rs`, hand-rolled INI, explicit on both sides

Precedence from AWS's SDK/CLI documentation (`03` §B): credentials from env
(`AWS_ACCESS_KEY_ID`+`AWS_SECRET_ACCESS_KEY`[+`AWS_SESSION_TOKEN`]) else the profile
(`AWS_PROFILE`, default `default`; credentials file beats config file); region `AWS_REGION` >
`AWS_DEFAULT_REGION` > profile `region` > `us-east-1`; endpoint `AWS_ENDPOINT_URL_S3` >
`AWS_ENDPOINT_URL` > profile `services` section `s3 = endpoint_url` > profile `endpoint_url`.
Files from `AWS_SHARED_CREDENTIALS_FILE`/`AWS_CONFIG_FILE`, else `~/.aws/credentials` and
`~/.aws/config`. INI parsing hand-rolled (~120 lines): crates flatten the indented sub-keys of the
`services` form. Path-style addressing whenever an endpoint override is set, virtual-hosted on
AWS's default endpoint, stated explicitly to both consumers. Fail closed on half-set env
credentials, an endpoint carrying a path, and no credentials at all. Mapping: `http://h:p` →
DuckDB `ENDPOINT 'h:p'`, `USE_SSL false`, `URL_STYLE 'path'`; object_store `with_endpoint(url)`,
`with_allow_http(true)`, `with_virtual_hosted_style_request(false)`.

## D5 — Proxy variables are ignored by both directions

DuckDB 1.4.4 ignores `HTTP(S)_PROXY`/`NO_PROXY` (measured, `04` §8); reqwest honours them. If only
one side honoured them, the writer and the readers could reach different endpoints behind a proxy.
Both go direct for the pilot, the man pages say so, and a follow-up seed
(`issues/s3-proxy-support.md`) carries one shared proxy setting.

## D6 — Loopback endpoint: s3s-fs 0.17.0 in the test process

MinIO's repository is archived and its last binaries are from 2025-09; versitygw 1.10.0 sent no
body for whole-object GETs on darwin/arm64; moto accepts any secret by default and, in IAM mode,
rejected DuckDB's valid signatures. s3s-fs 0.17.0 (Apache-2.0) verifies SigV4 (403
`SignatureDoesNotMatch` on a wrong secret), serves Range, ListObjectsV2, HEAD and DELETE, and runs
in-process as a dev-dependency: `S3ServiceBuilder::new(FileSystem::new(dir)).set_auth(…)` on
`127.0.0.1:0`, one per test. Pinned through `Cargo.lock`, no CI install step, never absent. MSRV
1.96 against the 1.97.1 pin. Its root directory is the bucket's contents as plain files (probed
here): a test seeds an index by copying a local one under `<root>/<bucket>/<prefix>/`, and compares
a writer's output against a local run by walking the same directory.

**Sandbox:** this host's sandbox lets a process bind and listen on `127.0.0.1` and another process
connect to it (Python socket, `http.server`, s3s-fs, moto all served loopback here). It refuses
signals to a process started by an earlier command, so a test server must be a child of the test
process (in-process satisfies this by construction).

## D7 — R3 is measured by a std-only counting proxy in `tests/common`

The proxy sits between the binary under test and the endpoint; the binary sees only the proxy's
address (`AWS_ENDPOINT_URL=http://127.0.0.1:<proxy>`), so traffic that bypasses it cannot reach the
data. It passes `Host` through unchanged (SigV4 signs it). It counts every byte sent to the client,
headers and LIST included (over-counting is the conservative direction), and parses request lines
and `Range` headers from the client side. Assertions (hardened in `06`): the set of range-fetched
keys equals the set of chunk keys; every GET of a `.parquet`
key carries a `Range` shorter than the object; total bytes < 5% of the stored bytes, summed from the
endpoint's root directory, not from the proxy. A negative control drives a query that must read
whole columns and asserts the meter crosses the threshold, so a meter that sees nothing cannot pass.
DuckDB's own counters are rejected: `EXPLAIN ANALYZE` disagreed with the wire, and `duckdb_logs`
rows carry the `Authorization` header. Fixture: each chunk costs a fixed ~16–33 KiB footer read, so
a ≥10 MiB fixture keeps ≤12 large chunks with incompressible paths, written by test code through
`lib::get_schema()` rather than by crawling ~150k files.

## D8 — Bounded marker read: size first, then content, two statements

`SELECT size FROM read_blob(?)` is one HEAD and no GET, even for a 2 MiB object; `SELECT content`
is one GET of the whole object; a single statement with `CASE` on size still fetches it all (`04`
§5). So: size, refuse above `MARKER_READ_LIMIT`, then content. The gap between the two equals
today's local stat-then-read gap and only a concurrent writer (a non-goal) can exploit it. Errors
must be classified by the tool: a missing key surfaces as a 404 whose text says "Unable to
connect", a wrong secret as `HTTP 403 Forbidden`, a dead port as `Could not establish connection`,
an empty listing as `No files found`. A `glob(?)` LIST before the HEAD separates "endpoint or
credentials" (LIST fails) from "marker absent" (LIST succeeds, key missing).

## D9 — Writer ports the archive's P1 seam, with five changes

`IndexSink` + `LocalSink` + `S3Sink` from `32ba641` port (`02`). Changes: keep `-o` a `PathBuf` and
classify on the `OsStr` (the archive's `String` broke non-UTF-8 local paths); the resolver replaces
`--s3-endpoint`/`XDU_S3_*`/`from_env()`; pass the encoded `Vec<u8>` by value; refuse partition
names that do not survive as an S3 key and a DuckDB glob (non-UTF-8, names object_store would
percent-encode) before the marker clear; a bounded retry. Order: LIST pre-flight → every check that
can reject → DELETE marker (the write probe: read-only credentials fail here with the marker intact)
→ PUT chunks at final keys → LIST + DELETE every higher id → PUT marker. The prune deletes past a
gap where local stops at it; the gap only arises from a foreign edit, and deleting it serves R1's
"no stale chunk". Peak memory ~16 MB per driver.

## D10 — Tests: tiers, hermetic children, and a skip that cannot hide

- **Always run:** locator and resolver unit tests, R8 refusal, R7 absent-extension text, and "a
  local query loads nothing even with a garbage httpfs in `XDU_EXTENSION_DIR`".
- **Need the pinned httpfs:** everything over the wire. Marked `#[ignore = "…"]`, so plain
  `cargo test` lists them as ignored. CI fetches httpfs with `packaging/extensions/fetch.sh` into
  `$RUNNER_TEMP` (never `target/lib/xdu/extensions`, the default lookup that would hide a reader
  wrongly loading httpfs for a local index), exports `XDU_TEST_EXTENSION_DIR`, and runs
  `--include-ignored`; once opted in, a missing extension or a refused bind panics. A CI guard
  asserts the S3 step's passed count equals its `--list --include-ignored` count (under
  `--include-ignored` libtest always prints `0 ignored`, so that phrase proves nothing — `06` #1).
- **Hermetic children:** every spawned binary gets all `AWS_*` and `*_PROXY` removed, a temp `HOME`,
  and fixture credential/config files.
- Loading httpfs from a debug build has been observed only on macOS; the first CI push proves
  linux_amd64.

## D11 — One CA file, chosen by the resolver, verifies both directions (P6, R11)

From [`07`](07-tls-server-auth.md). The pinned httpfs reads S3 through httplib with OpenSSL 3.0.8
linked in, and verifies nothing unless `enable_server_cert_verification` is on. Without
`ca_cert_file` it trusts OpenSSL's defaults: `SSL_CERT_FILE`, else the compiled-in `/etc/ssl`,
which misses RHEL-family hosts. The writer trusts `rustls-platform-verifier`: the macOS keychain,
or the native store, which honours `SSL_CERT_FILE` too. So the resolver picks one file,
`AWS_CA_BUNDLE` else the first platform bundle present, in httpfs's own probe order, and checks
it holds a usable certificate before any request. The reader pins httplib, turns verification on
and binds the path as `SET ca_cert_file = ?`; the writer's client gets `tls_certs_only` of the
same file's certificates. A plain-`http://` endpoint resolves no file. Real roots on the release
platforms are checked by a CI request to AWS with the documented example key pair, which only
roots, not verification, can fail.
