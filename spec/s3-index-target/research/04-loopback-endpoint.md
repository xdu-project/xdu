# Research 04: a loopback S3 endpoint with no container runtime

Track: R10's endpoint, R3's measurement, and R4's credential tests. Measured 2026-11-05 on
darwin/arm64 with Python `duckdb==1.4.4` (the engine the crate bundles) and the official
`v1.4.4/osx_arm64` `httpfs`, loaded from a scratch `extension_directory` under a cold `HOME`. A
Python stdlib counting proxy sat between DuckDB and each server and logged method, path, `Range`,
status and body bytes. Each run started its own server and stopped it. The fixture was 4 partitions
(`__root__`, alice, bob, carol) x 2 chunks, written with `COPY … TO 's3://…'`: 22.1 MiB in 8
objects, plus a 9-byte marker and a 2 MiB marker under another prefix.

## 1. Candidates

| candidate | install on both CI OSes | pin / integrity | start | SigV4 verified | Range, ListV2 prefix/delimiter/continuation, HEAD, DELETE | license | verdict |
|---|---|---|---|---|---|---|---|
| MinIO | none current | last release with binaries is `RELEASE.2025-09-07`; `2025-10-15` shipped source only; repo **archived** | n/a | yes | yes | AGPL-3.0 | dead upstream; drop |
| versitygw 1.10.0 (posix) | release tarballs for Linux x86_64 and Darwin arm64/x86_64 | `checksums.txt` (sha256) | 0.06 s | **yes** (403 `SignatureDoesNotMatch`) | all pass, but see below | Apache-2.0 | **broken on macOS**: every whole-object GET failed |
| SeaweedFS 4.52 (`weed mini`/`server -s3`) | tarballs for both OSes | `.md5` only | not run | configurable | not run | Apache-2.0 | heavier; weak checksum story |
| rustfs 1.0.4 | Linux gnu/musl, macOS aarch64 zips | SHA256SUMS + provenance | not run | yes | not run | Apache-2.0 | heavyweight distributed store for a test fixture |
| Garage | needs a config file and a layout step before it serves | — | not run | yes | — | AGPL-3.0 | too much setup |
| moto 5.2.5 server (`uvx --from 'moto[server]'`) | uv on both OSes; PyPI | version pin; uv lock hashes | 0.3–0.7 s (warm cache) | **no** by default: a wrong secret was accepted by boto3 and by DuckDB | all pass | Apache-2.0 | can't test R4; see below |
| **s3s-fs 0.17.0** (s3s project) | `cargo install s3s-fs --version 0.17.0 --features binary --locked` (37 s cold here), or used **in-process as a dev-dependency** | crates.io checksums + `--locked` / our `Cargo.lock` | 0.06 s | **yes** (403 `SignatureDoesNotMatch`) | all pass | Apache-2.0 | **recommended** |
| in-repo minimal server (std-only Rust or Python) | nothing to install | in-tree | instant | only if we hand-roll SHA-256/HMAC | only what we write | ours | circular: it tests our client against our own reading of S3 |

Fidelity findings:

- **versitygw on darwin/arm64:** a GET covering a whole object failed for every size tried, from
  64 KiB to 2.9 MiB. It was a `206`/`200` with `Content-Length` set and zero body bytes, with boto3
  and DuckDB alike. Partial ranges of the same objects worked, so DuckDB's chunk reads, including a
  20 MiB `path LIKE`, passed. The 9-byte marker read also passed. `--nometa`,
  `--disable-copy-file-range` and a larger `--io-buffer-size` changed nothing. Not root-caused, and
  Linux was not tried. One CI leg would test a different server than the other, so versitygw is out.
- **moto:** with no auth configured it accepts any secret. With `INITIAL_NO_AUTH_ACTION_COUNT` IAM
  mode it rejected DuckDB's **valid** signatures (403 on both LIST and ranged GET) while it accepted
  boto3's. An unsigned (empty-key) LIST also returned 200 while a GET returned 403. So moto can't
  carry R4, and it puts Python on the Rust test path.
- **s3s-fs:** after LIST, DuckDB issues one HEAD per object (8 HEADs, which moto and versitygw did
  not draw), so request counts depend on the endpoint. Assert on bytes and ranges, never on
  request counts. It also served DuckDB's multipart `COPY`, boto3 single PUTs with CRC32 checksums,
  and full and partial GETs at every size.

## 2. Hands-on: DuckDB 1.4.4 `httpfs` against moto, versitygw and s3s-fs

The results were the same on all three servers except where noted. Settings used:
`s3_endpoint='127.0.0.1:PORT'`, `s3_use_ssl=false`, `s3_url_style='path'`, region, key and secret.

**a. `count(*)` over `idx/*/*.parquet`:** 1 LIST (`GET /bucket/?list-type=2&prefix=idx%2F`) plus
one GET per chunk, each `Range: bytes=<size-16384>-<size-1>` → 206 with 16,384 bytes. There were
**no unranged GETs** of any object in any query on any server. Received 132.5–134.1 KB of
23.2 MB, **0.57–0.58 %** (R3 needs < 5 %). One partition: 1 LIST + 2 GETs, 33 KB. A `path LIKE`
scan: 16 ranged GETs, 20.7 MB (the worst case, as in the spike). `sum(size)`: 16 ranged GETs,
1.4 MB.

**b. Bounded marker read.** This decides R2's cap:

- `SELECT size FROM read_blob(?)` issues **one HEAD and no GET**, for the 9-byte marker and the
  2 MiB one alike. `filename` and `size` together also cost only a HEAD.
- `SELECT content FROM read_blob(?)` (or `read_text`) issues a HEAD then **one GET ranged
  `bytes=0-<size-1>`**, which fetches the whole object in one request.
- `SELECT size, CASE WHEN size <= cap THEN content END FROM read_blob(?)` **still GETs the full
  2 MiB** and then returns NULL. The gate can't share a query with the read.
- So the cap can be done in DuckDB as **two statements**: read `size` (HEAD only), refuse if it is
  over the cap, then read `content`. The remaining gap is a marker swapped for a larger object
  between the two statements, because the second HEAD sets the range. That needs a concurrent
  writer, which is a non-goal.
- A missing marker key gives `HTTPException: HTTP Error: Unable to connect to URL "…/.xdu-complete":
  404 (Not Found).` after one HEAD and no retry. The wording says "connect", so the tool should
  map a 404 here to "marker missing" itself.
- A glob over a missing prefix gives a clean `IO Error: No files found that match the pattern
  "s3://…"` (1 LIST).
- A missing bucket gives a 404 on the LIST, retried 4 times (about 0.5 s), with a garbled path in
  the message (`s3://no-such-bucket/idx/no-such-bucket/?…`).

**c. Wrong secret** (versitygw, s3s-fs): 403 on the LIST, 4 attempts, about 0.5 s:
`HTTP Error: HTTP GET error reading 's3://…' in region 'us-east-1' (HTTP 403 Forbidden)`. On the
marker, the HEAD returns 403, then 4 ranged GETs return 403. Empty credentials also give 403. An
unreachable port gives `IO Error: Could not establish connection error for HTTP GET to '/bucket/?…'`
after about 0.5 s. All of these errors are clean and fast, and none leaks the secret.

**d. DuckDB's own accounting.**

- `EXPLAIN ANALYZE` prints an "HTTPFS HTTP Stats" block (`in: 128.0 KiB`, `#HEAD`, `#GET`), but the
  numbers are rounded, leave out LIST bytes, and **disagree with the wire**. With versitygw it
  printed `#GET: 10` against 9 seen. With s3s-fs it printed `#HEAD: 9, #GET: 8` against 8 HEADs
  and 9 GETs seen.
- `CALL enable_logging('HTTP')` then `duckdb_logs` gives one row per request, with request
  `Range` and response `Content-Range`/`Content-Length`. The rows also carry the `Authorization`
  header (key id and signature), which runs against R5's no-secret-in-output rule if it ever
  surfaced.
- Both come from the process under test and need it instrumented. Neither should be the CI measure.

**Proxy behaviour:** DuckDB 1.4.4 `httpfs` **ignores `HTTP_PROXY`** in the environment. It went
straight to loopback even with `NO_PROXY` removed. boto3 honours the variable, and so do most Rust
HTTP stacks. Tests should `env_remove` the `*_PROXY` variables (or set `NO_PROXY=127.0.0.1`) on
every spawned binary so a developer's proxy can't swallow loopback traffic. SigV4 survives a proxy
hop **only if `Host` is forwarded unchanged**. DuckDB signed `Host: 127.0.0.1:<proxy port>`, and
versitygw and s3s-fs verified it.

## 3. R3 measurement for CI

Use **a std-only counting proxy in `tests/common`** that forwards to the endpoint. Endpoint-side
counters tie the test to one server's internals, and DuckDB's stats fail on accuracy and
independence (§2d). The test hands `xdu-find` the proxy as its endpoint
(`AWS_ENDPOINT_URL=http://127.0.0.1:<proxy port>`, through R5's resolver). The writer and fixture
setup talk to the endpoint directly, as in the spike. The proxy works like this:

- One thread per connection.
- It reads the request head and any `Content-Length` body, and forwards them with `Host` untouched
  and `Connection: close`.
- It reads the upstream response to EOF, records `{method, path, query, range, status, body_bytes}`,
  and relays the response with `Connection: close`.
- Closing per request avoids parsing keep-alive framing. DuckDB's handling of `Connection: close`
  is standard HTTP but was not tried here; the Python proxy kept connections alive.

The R3 test then asserts:

1. **Positive control:** at least one request seen, and every chunk object touched. A run that
   bypassed the proxy would otherwise pass with 0 bytes.
2. Every GET of a `*.parquet` key carries `Range: bytes=a-b` with 206, **and** `b-a+1` is below
   that object's size. "Ranged" alone is not enough, because `read_blob` asks for
   `bytes=0-<size-1>`.
3. The sum of body bytes over all logged requests, LIST included, is under 5 % of stored bytes.
   Stored bytes are the chunk sizes listed directly from the endpoint, not taken from the proxy.

Fixture sizing:

- Cost is fixed per chunk: 16 KiB in this probe, and about 33 KiB per chunk on the spike's real
  index.
- So `N_chunks x ~33 KiB` must stay well under 5 % of the index size. At 10 MiB that means
  **≤ ~12 chunks**, which leaves ample room at 8.
- Make the 10 MiB from few, large, poorly-compressible chunks. A synthetic index in xdu's schema
  written by test code is far cheaper than crawling roughly 150k files.
- Asserting byte ratios, not counts, keeps the test valid across endpoints (§1, s3s-fs HEADs).

## 4. Skip semantics (R10)

The recommended endpoint is **compiled into the tests**, so it can't be absent and nothing ever
skips.

If the plan instead spawns an external `s3s-fs` binary:

- Mark the S3 tests `#[ignore = "s3: needs a loopback endpoint (XDU_TEST_S3_BIN)"]`. libtest then
  prints `N ignored` and the reason, which is a real skip, unlike an early `return` that shows `ok`.
- CI runs them with `-- --include-ignored` and `XDU_TEST_S3_REQUIRED=1`. Under that variable the
  endpoint helper **panics** when the binary is missing or fails to start.
- A CI step also greps the libtest summary for `0 ignored` and a non-zero passed count for that
  test target, so CI can't silently skip.

## 5. CI provisioning

**Recommended, in-process:**

- Add dev-dependencies pinned exactly: `s3s = "=0.17.0"`, `s3s-fs = "=0.17.0"`, plus `tokio`
  (rt-multi-thread, net) and `hyper-util` (server, tokio).
- `tests/common` starts `S3ServiceBuilder::new(FileSystem::new(tempdir)).set_auth(SimpleAuth::from_single(k, s))`
  on `127.0.0.1:0` in a background runtime, one server per test. This copies `s3s-fs`'s own
  `main.rs`, about 30 lines.
- No workflow step is needed on either leg. `rust-cache` caches the roughly 200 extra crates.
  MSRV 1.96 is within the pinned 1.97.1.
- Add one guard step:

```yaml
      - name: S3 loopback tests ran (none ignored)
        run: |
          set -eu
          out=$(cargo test --locked --all-features --test s3_tests 2>&1) || { printf '%s\n' "$out"; exit 1; }
          printf '%s\n' "$out"
          printf '%s\n' "$out" | grep -Eq 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'
```

**Fallback, external binary, both legs:**

```yaml
      - uses: actions/cache@<pinned-sha>
        with:
          path: ~/.local/s3s-fs
          key: s3s-fs-0.17.0-${{ runner.os }}-${{ runner.arch }}
      - name: Install loopback S3 endpoint (s3s-fs 0.17.0, crates.io checksums, --locked)
        run: test -x ~/.local/s3s-fs/bin/s3s-fs || cargo install s3s-fs --version 0.17.0 --features binary --locked --root ~/.local/s3s-fs
      - name: S3 tests (absence is a failure here)
        env:
          XDU_TEST_S3_REQUIRED: "1"
        run: XDU_TEST_S3_BIN="$HOME/.local/s3s-fs/bin/s3s-fs" cargo test --locked --all-features --test s3_tests -- --include-ignored
```

## Recommendation

- **Endpoint:** `s3s-fs` 0.17.0 (Apache-2.0), in-process as a dev-dependency. It verifies SigV4,
  so R4's rejected-credentials case is testable. It served every operation the readers and a
  single-PUT writer need, on loopback only, with no container, no download step and no Python. It
  is pinned and checksummed through `Cargo.lock`.
- **Measurement:** a std-only counting proxy in `tests/common`, with the three assertions in §3.
- **Skip scheme:** none needed in-process, plus the guard step against a future `#[ignore]`. For
  an external binary: `#[ignore]`, `--include-ignored`, `XDU_TEST_S3_REQUIRED=1`, and the summary
  grep.

## Risks

- **s3s-fs fidelity:** it is a reference backend, not a production store. Its LIST draws per-object
  HEADs from DuckDB that AWS and MinIO don't. Its multipart, checksum and error-body details may
  differ from AWS. The GOAL accepts loopback as the whole acceptance, and a real-provider run
  remains demo preparation.
- **Dev-dependency weight:** tokio and hyper enter the test build, about 200 crates and 40 s cold.
  If the writer's S3 client already brings tokio and hyper, the extra cost is small. A 0.x crate
  may churn its API, so pin it with `=`.
- **Marker cap:** two statements, with a swap window between them (§2b). A strict bound would need
  a ranged GET from the Rust client (`bytes=0-<cap>`), which the readers otherwise don't need.
- **DuckDB retries 403/404 four times** (about 0.5 s each case). That is fine for R4 tests, but
  keep the timeouts generous.
- **versitygw on macOS** may be fixed upstream later. It stays excluded until both legs run one
  server.
