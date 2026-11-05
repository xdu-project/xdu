# Research 03: writer transport and the one credential resolver

Track: how `xdu -o s3://…` moves bytes (R1, R4), the Rust resolver both directions share (R5), how
its result maps into DuckDB and the client (R5, R6, R7), and what R9 must name. This adopts archived
brief `04-rust-s3-client.md` (object_store > aws-sdk-s3 > opendal) and its prior art at `32ba641`
(`src/s3.rs`: a `Sink` trait, `S3Sink` over an owned runtime, atomic PUT at final keys, LIST prune,
marker-as-commit). Only the deltas are verified here. Dependency counts come from `cargo tree -e normal`
on throwaway crates, `--target x86_64-unknown-linux-gnu`, run 2026-11-05.

## A. Writer transport: a Rust client, not DuckDB

**DuckDB cannot be the writer.** Three findings, each sufficient on its own:

1. **Fidelity.** httpfs writes only what `COPY … TO 's3://…'` produces, so every chunk would be
   re-encoded by DuckDB's Parquet writer (its own compression defaults, row-group sizing and
   nullability) instead of the arrow-rs `ArrowWriter` output a local run writes. R1 says "the same
   index". The C API that the `duckdb` crate binds (libduckdb-sys 1.4.4) exposes no file-system or
   file-handle functions, so pre-encoded bytes cannot be handed to httpfs verbatim.
2. **No DELETE reachable from SQL.** httpfs v1.4 does implement `S3FileSystem::RemoveFile`, a DELETE
   (`src/s3fs.cpp:947`, branch `v1.4-andium`), but nothing in SQL calls it for a remote path. The
   only caller, `COPY … (OVERWRITE)`, refuses remote targets in the vendored 1.4.4 engine:
   `throw NotImplementedException("OVERWRITE is not supported for remote file systems")`. **So
   DuckDB alone cannot clear the §2b marker or prune stale chunks, and R1 cannot be met without a
   Rust client.**
3. **Crawl hot path.** The GOAL already rules out re-encoding through `COPY`.

**The writer's operations:** PUT (chunk and marker, each a single atomic object), LIST of a prefix
(pre-flight inventory and stale-chunk prune), and DELETE (marker clear, prune). The auth and
reachability check can be the pre-flight LIST itself. It proves endpoint, bucket and credentials,
but not write permission, so the plan should decide whether to add a PUT+DELETE of a probe key
before the marker is cleared (R1: "accepted credentials").

### Candidates (normal deps, Linux target; xdu today has 165 normal / 349 lockfile crates)

| Option | total deps | new to Cargo.lock | crypto / TLS | MSRV | sync fit |
|---|---|---|---|---|---|
| object_store 0.14.2 `aws` (archived set) | 131 | 28 | **aws-lc-rs + aws-lc-sys**, rustls, reqwest 0.13, hyper, h2, tokio | 1.85 | owned tokio runtime |
| **object_store 0.14.2 `aws-base,reqwest,ring`** + reqwest `rustls-no-provider` + rustls `ring` | 130 | 26 | **ring only**, rustls, reqwest 0.13, tokio | 1.85 | owned tokio runtime |
| aws-sdk-s3 1.157.0 (default + `behavior-version-latest`) | 165 | 76 | aws-lc-rs **and** ring (rustls 0.21 and 0.23), smithy runtime | **1.94.1** | owned tokio runtime |
| rust-s3 0.38.0 `sync-rustls-tls` | 91 | 30 | aws-lc-rs via attohttpc's rustls | undeclared | native sync |
| hand-rolled SigV4: ureq 3.5.1 + hmac 0.13 + sha2 0.11 + quick-xml 0.42 | 40 | 15 | ring, rustls, webpki-roots | 1.88 (sha2) | native sync |

All MSRVs sit inside the pinned 1.97.1. The ring variant of object_store was checked with
`cargo check` and compiles. Its code paths are first-class (`src/client/crypto.rs:95`,
`#[cfg(all(feature = "ring", not(feature = "aws-lc-rs")))]`).

**Release-build risk.** Linux legs build inside `manylinux_2_28` and assert a GLIBC 2.28 floor plus
a NEEDED allow-list (libc, libm, libdl, libpthread, libgcc_s, libstdc++, ld-linux). Any
`native-tls`/OpenSSL option fails that allow-list on `libssl.so` and is out. ring needs only a C
compiler, and it **already builds in those containers today** as a build-dependency of
libduckdb-sys (via reqwest 0.12 + rustls). aws-lc-sys non-FIPS needs a C/C++ compiler and, per its
docs, "CMake and Go are only required for FIPS builds"
(<https://aws.github.io/aws-lc-rs/requirements/linux.html>). It is still a second large C crypto
build with no evidence yet from this repo's release legs, on top of the bundled-DuckDB full-LTO
compile. **Prefer ring:** it adds one crypto stack the containers have already proven, not two.

**reqwest's ring path needs an explicit provider.** With `rustls-no-provider`, reqwest 0.13.5
looks for an installed process default and otherwise panics (`async_impl/client.rs:2499`). Under
`panic = "abort"` that panic kills the process. Call
`rustls::crypto::ring::default_provider().install_default()` once, before building the store
(ignore the `Err` that means one is already installed).

**Offline.** None of the candidates downloads anything at run time. object_store falls through to
IMDS (`http://169.254.169.254`, `aws/builder.rs:1243`) **only when no static key/secret is set**,
and `AmazonS3Builder::from_env()` (used at `32ba641`) reads `AWS_*` itself, endpoint keys included.
So use `AmazonS3Builder::new()` and always pass the resolver's key and secret. The resolver must
refuse before `build()` when it finds no credentials, which keeps IMDS unreachable (R5 Non-goals).

**Proxy coherence (risk).** reqwest honors `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY`
(either case) by default (`auto_sys_proxy: true`; hyper-util `matcher.rs:231-234`), and object_store
never turns that off (`client/mod.rs:867-899`). DuckDB 1.4.4 reads proxy settings only from its
`http_proxy` config or from an explicit `CREATE SECRET (TYPE http, PROVIDER env)`, which reads
`http_proxy` alone and has no `NO_PROXY` (vendored source, `CreateHTTPSecretFromEnv`). Behind a
proxy, then, the writer could go through it while the readers go direct. **Recommendation:** for
the pilot, both go direct. Give object_store a custom `HttpConnector` that returns
`HttpClient::new(reqwest::Client::builder().no_proxy()…build())` (reqwest `Client` implements
object_store's `HttpService`, `client/http/connection.rs:220`). Re-apply the timeouts there, because
a custom connector bypasses `ClientOptions`. State in the man pages that proxy variables are not
used, and file a follow-up for a single resolver-chosen proxy fed to both sides. ureq has the same
env-proxy default (`Proxy::try_from_env()`).

**TLS roots.** reqwest 0.13 with rustls verifies against the platform trust store
(rustls-platform-verifier), which suits a site with a private CA. ureq's default is bundled
webpki-roots; it would need `platform-verifier`. DuckDB has its own `ca_cert_file`. A custom CA
across both sides is a follow-up and outside the loopback-only acceptance.

**Sync bridge.** As at `32ba641`: the S3 sink owns a `tokio::runtime::Runtime` (multi-thread, two
workers, `enable_all`), and driver `std::thread`s call `rt.block_on(store.put(…))`. `block_on` takes
`&self` and is safe from several non-runtime threads at once. Rayon workers are not tokio threads,
so nothing re-enters the runtime. Chunks are encoded into a `Vec<u8>` and passed as `Bytes` (zero
copy). They are never written to local disk, which satisfies R1.

**Recommendation:** **object_store 0.14.2 on ring**: atomic `put`, `list`, `delete`, and the
`InMemory` backend for unit tests against the same trait. **Fallback:** a hand-rolled SigV4 over
ureq (15 new crates, no runtime, ring only). It costs about 400–600 lines with tests (canonical-URI
encoding, ListObjectsV2 pagination and XML), and it is the exit if the tokio bridge or reqwest's
proxy and TLS defaults misbehave. aws-sdk-s3 (76 new crates, two crypto stacks, MSRV that chases
stable) and rust-s3 (aws-lc, undeclared MSRV) are dominated.

## B. The resolver (R5), in Rust

AWS-documented precedence:

- **Credentials.** `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ `AWS_SESSION_TOKEN`) override
  profile values (<https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-envvars.html>).
  Otherwise use the selected profile. "If there are credentials in both files for a profile sharing
  the same name, the keys in the credentials file take precedence"
  (<https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-files.html>).
- **Profile.** `AWS_PROFILE`, else `default`. Section names are `[default]` / `[profile NAME]` in
  the config file and `[default]` / `[NAME]` in the credentials file. The file paths are
  `AWS_SHARED_CREDENTIALS_FILE` (default `~/.aws/credentials`) and `AWS_CONFIG_FILE` (default
  `~/.aws/config`).
- **Region.** `AWS_REGION` > `AWS_DEFAULT_REGION` > profile `region` in the config file. CLI v2
  checks both variables and prefers `AWS_REGION`
  (<https://docs.aws.amazon.com/sdkref/latest/guide/feature-region.html>). AWS's default is "None".
- **Endpoint.** `AWS_ENDPOINT_URL_S3` > `AWS_ENDPOINT_URL` > `endpoint_url` under `s3 =` in the
  `[services NAME]` section the profile names via `services = NAME` > profile `endpoint_url`. Valid
  values are "a URL including the scheme and host … optionally … a path"
  (<https://docs.aws.amazon.com/sdkref/latest/guide/feature-ss-endpoints.html>).

**Pilot subset:** everything above, plus three fail-closed rules:

1. Partial env credentials (a key without a secret, or the reverse) are an error, not a silent
   fallthrough.
2. An endpoint with a non-root path is refused, because httpfs places `s3_endpoint` directly in the
   Host (`s3fs.cpp:695-714`).
3. No credentials at all is an error. That error is what keeps the client off IMDS.

Skip `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS`, `source_profile`/`role_arn`, `credential_process`,
`sso_*` and IMDS (Non-goals). When no region resolves, default it explicitly to `us-east-1` and pass
it to both sides. object_store defaults there, but httpfs' default for an empty region is
`s3.amazonaws.com`, so leaving it implicit would let the two disagree.

**Addressing style.** The CLI's `s3 =` → `addressing_style = path|virtual|auto` (default `auto`) is
CLI-specific. Rule: **path style whenever an endpoint override is present, virtual-hosted on AWS's
default endpoint**, set explicitly on both sides. object_store with a custom endpoint and
virtual-hosted style uses the endpoint verbatim (`builder.rs:1294`), so the bucket would vanish;
path style is the only correct choice for overrides. Honoring `addressing_style` from the profile is
an optional, cheap extra once nested keys parse.

**INI: hand-roll it** (about 120 lines plus tests). The AWS format's indented sub-keys
(`s3 =` / `  endpoint_url = …`) are not standard INI. rust-ini 0.21.3 (+3 crates) and configparser
3.3 flatten them, and they matter here for the services section. Handle `#`/`;` comments, `[default]`
vs `[profile default]`, case-sensitive profile names, and whitespace trimming.

## C. Mapping the resolved value

`http://127.0.0.1:9000` → scheme `http` → `s3_use_ssl=false`; authority `127.0.0.1:9000` →
`s3_endpoint` (no scheme: httpfs prepends `http://`/`https://` from `s3_use_ssl`, `s3fs.cpp:714`;
the doc example is `SET s3_endpoint = 'domain.tld:port'`,
<https://duckdb.org/docs/1.4/core_extensions/httpfs/s3api_legacy_authentication.html>); plus
`s3_url_style='path'`. Keep IPv6 brackets.

| Resolved | DuckDB `Config` key (1.4) | object_store 0.14 builder |
|---|---|---|
| key / secret / token | `s3_access_key_id`, `s3_secret_access_key`, `s3_session_token` | `with_access_key_id`, `with_secret_access_key`, `with_token` |
| region | `s3_region` | `with_region` |
| endpoint authority | `s3_endpoint` (host[:port]) | `with_endpoint("<scheme>://<authority>")` |
| scheme http | `s3_use_ssl=false` | `with_allow_http(true)` |
| style | `s3_url_style` = `path`/`vhost` (default `vhost`) | `with_virtual_hosted_style_request(false/true)` (default path) |
| no env auto-load | see risk 1 | `AmazonS3Builder::new()`, never `from_env()` |
| no IMDS | n/a (no `aws` extension) | always static creds; resolver errors otherwise |

## D. Names R9 must document

`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, `AWS_REGION`,
`AWS_DEFAULT_REGION`, `AWS_PROFILE`, `AWS_SHARED_CREDENTIALS_FILE`, `AWS_CONFIG_FILE`,
`AWS_ENDPOINT_URL`, `AWS_ENDPOINT_URL_S3`. The files `~/.aws/credentials` and `~/.aws/config`
(including `region`, `endpoint_url` and the `services` → `s3` form). `XDU_EXTENSION_DIR` for
`xdu-find`/`xdu-view`. A sentence that proxy variables are not used (per A).

## Recommendation

| Option | R1 fidelity | PUT/LIST/DELETE | dep weight (new lock crates) | manylinux risk | offline | proxy coherence |
|---|---|---|---|---|---|---|
| DuckDB `COPY` | **fails** (re-encodes) | PUT only; **no DELETE** | 0 | none | yes | n/a |
| **object_store 0.14.2, ring** | exact bytes | all | 26 | low (ring proven) | yes, with static creds + `new()` | fixable: `no_proxy()` connector |
| object_store 0.14.2, `aws` | exact | all | 28 | medium (aws-lc-sys unproven here) | same | same |
| aws-sdk-s3 1.157 | exact | all | 76 | medium (aws-lc + ring) | must disable IMDS chain | not verified |
| rust-s3 0.38 sync | exact | all | 30 | medium (aws-lc) | static creds | env default (attohttpc) |
| ureq + SigV4 | exact | all, hand-written | 15 | low (ring) | yes | `.proxy(None)` |

Cargo.toml:

```toml
object_store = { version = "0.14.2", default-features = false, features = ["aws-base", "reqwest", "ring"] }
reqwest = { version = "0.13", default-features = false, features = ["rustls-no-provider"] }
rustls = { version = "0.23", default-features = false, features = ["ring", "std"] }
tokio = { version = "1", features = ["rt", "rt-multi-thread"] }
futures = "0.3"
bytes = "1"
```

## Open risks

1. **httpfs re-reads AWS env on `LOAD`.** `AWSEnvironmentCredentialsProvider::SetAll` runs at load
   (`httpfs_extension.cpp:122`) and calls `config.SetOption` from `AWS_REGION`/`AWS_DEFAULT_REGION`,
   the `AWS_*` keys, and also `DUCKDB_S3_ENDPOINT`/`DUCKDB_S3_USE_SSL`. This can overwrite `Config`
   values supplied at open. Env-first precedence keeps credentials consistent, but a stray
   `DUCKDB_S3_ENDPOINT` would split reader from writer. The plan should probe it, then re-apply the
   non-secret keys (endpoint, use_ssl, url_style, region) after `LOAD`. Those may go through
   SQL `SET` because they are not secrets.
2. The pre-flight LIST does not prove write permission (see A).
3. The `ring` provider install must precede every reqwest client build; a missed call aborts the
   process.
4. aws-lc-sys was never exercised in this repo's release legs. If the plan keeps `aws`, run the
   release workflow's PR path (it triggers on `Cargo.lock`) before build sign-off.
5. CI runners with a proxy set: with the `no_proxy()` connector, loopback MinIO tests are immune.
   Without it, tests need `NO_PROXY=127.0.0.1`.
