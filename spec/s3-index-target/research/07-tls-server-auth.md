# 07 — TLS server authentication for both directions (P6, R11)

Plan-time probes for review cycle 3 finding 1, run on macOS arm64 against the pinned `httpfs`
(`target/test-ext/httpfs.duckdb_extension`, DuckDB v1.4.4) and the locked writer stack. Every
server ran inside the one shell call that used it and stopped there. Nothing here touched a
network beyond loopback.

## Setup

- Certificates from the `openssl` CLI: a test CA (`CN=xdu test CA`, `CA:TRUE`), a leaf it signs
  for `IP:127.0.0.1`, and a self-signed certificate for `IP:127.0.0.1`. `openssl req -x509` marks
  its self-signed output `CA:TRUE` by default, which matters below.
- Server: Python 3.14's `http.server` behind `ssl`, two ports in one process, one per
  certificate, answering `HEAD` and ranged `GET` and logging each request.
- Reader probe: a throwaway example (moved out of the tree afterwards) that calls
  `engine::open_connection` and `engine::load_extension_from`, then runs SQL, optionally with one
  bound parameter, on that one connection. A fresh process per scenario, so no setting carries.
- Writer probe: a throwaway example building `reqwest` 0.13.5 on `rustls-no-provider` with ring
  installed, both as today (no root options) and with `tls_certs_only(Certificate::from_pem_bundle(file))`.

## Reader: what the pinned httpfs is

- **Two HTTP stacks, httplib by default.** `strings` shows `CLIENT libcurl 8.13.0-DEV` and
  `OpenSSL 3.0.8 7 Feb 2023` with `OPENSSLDIR: "/etc/ssl"`, both linked statically; `otool -L`
  shows no libssl or libcurl. `duckdb_settings()` lists `httpfs_client_implementation = default`,
  which accepts `curl`, `httplib` and `default`. `default` is httplib: with its settings untouched
  a self-signed server was accepted, while `SET httpfs_client_implementation = 'curl'` refused
  both servers (curl's own `enable_curl_server_cert_verification` defaults to `true`).
- **The settings are global `SET`s, not secret parameters.** `duckdb_settings()`:
  `enable_server_cert_verification = false` (GLOBAL, "Enable server side certificate
  verification."), `ca_cert_file` empty (GLOBAL, "Path to a custom certificate file for
  self-signed certificates."), `enable_curl_server_cert_verification = true` (GLOBAL). Each
  `open_connection` is its own in-memory database, so a `SET` reaches only that connection.
- **`SET ca_cert_file = ?` takes a bound parameter.** Prepared with the path bound,
  `current_setting('ca_cert_file')` read the path back. The path never needs to be SQL text.

| Scenario (httplib unless noted) | CA-signed leaf | Self-signed |
|---|---|---|
| Defaults | accepted | **accepted** (the finding) |
| `enable_server_cert_verification = true`, no `ca_cert_file` | refused, "SSL server verification failed" | refused |
| Verification on, `ca_cert_file` = test CA | accepted | refused |
| `ca_cert_file` = test CA, verification left off | — | **accepted**: the file alone verifies nothing |
| Verification on, `ca_cert_file` absent, or a CSR (no certificate) | refused, "SSL certificate loading failed", at request time | — |
| Verification on, no `ca_cert_file`, `SSL_CERT_FILE` = test CA | **accepted** | — |
| Verification on, `ca_cert_file = /etc/ssl/cert.pem`, `SSL_CERT_FILE` = test CA | refused | — |
| Verification on, `ca_cert_file` = a PEM with a key block before the CA | accepted | — |
| curl, no `ca_cert_file` | refused | refused |
| curl, `ca_cert_file` = test CA | **refused** | refused |
| curl, `enable_curl_server_cert_verification = false`, httplib flag on, test CA file | — | **accepted** |
| `s3://bkt/a.txt` through a `CREATE SECRET` with `USE_SSL true`, verification on, test CA bound | accepted | refused (`HTTP HEAD to 'https://127.0.0.1:…/bkt/a.txt'`) |
| The same `s3://` read with the settings untouched | — | **accepted** |
| Verification on, test CA bound, server shows a test-CA leaf for `DNS:wrong.example` only | refused, "SSL server verification failed": httplib checks the name too | — |

What follows:

- With no `ca_cert_file`, httplib uses OpenSSL's default verify paths: `SSL_CERT_FILE` and
  `SSL_CERT_DIR` when set, else the compiled-in `/etc/ssl` (`cert.pem`, `certs/`). That
  directory is right on macOS and Debian-family hosts and wrong on RHEL-family ones, whose bundle
  is under `/etc/pki`. The extension imports no `SecTrustCopyAnchorCertificates` or
  `SecTrustSettingsCopyCertificates` (`nm -u`), so httplib does not read the macOS keychain; its
  Security.framework imports belong to curl.
- With `ca_cert_file` set, that file is the only source: `SSL_CERT_FILE` stopped mattering.
- The curl backend ignores `ca_cert_file` and is governed only by its own flag, so a reader on
  curl could not be pointed at `AWS_CA_BUNDLE` at all. Pinning `httplib` keeps one source; a
  future engine whose `default` moves to curl cannot change it silently.
- The `s3://` path reads the same global settings as a plain `https://` read.
- **Confirmed:** always hand httpfs an explicit CA file, chosen by the resolver: `AWS_CA_BUNDLE`,
  else the first platform bundle present. The paths, in the order the extension's own curl client
  lists them (adjacent strings in its read-only data): `/etc/ssl/certs/ca-certificates.crt`,
  `/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem`, `/etc/pki/tls/certs/ca-bundle.crt`,
  `/etc/ssl/ca-bundle.pem`, `/etc/ssl/cert.pem`. httpfs reports a bad file only at the first
  request, so the resolver must check it first.

## Writer: where its roots come from

- `reqwest` 0.13.5's `rustls-no-provider` feature brings `rustls-platform-verifier` 0.7.1. With
  no root option, `ClientBuilder::build` uses `rustls_platform_verifier::Verifier::new`:
  Security.framework on macOS (the keychain), and on Linux the native store that
  `rustls-native-certs` loads, which honours `SSL_CERT_FILE` and `SSL_CERT_DIR`. `webpki-roots`
  is in the lockfile but not on this path. `S3Sink`'s `DirectConnector` builds its own
  `reqwest::Client`, so `object_store`'s `ClientOptions` root settings never reach it.
- `tls_certs_only(certs)` swaps the platform verifier for a `WebPkiServerVerifier` over exactly
  those roots. `tls_certs_merge` would add them to the platform's.

| Writer client | CA-signed leaf | Self-signed |
|---|---|---|
| Today (platform verifier) | refused, `OSStatus -26276` | refused |
| `tls_certs_only(test CA)` | accepted (200) | refused, `CaUsedAsEndEntity` |
| `tls_certs_only(/etc/ssl/cert.pem)`, 128 certificates, all accepted by `RootCertStore::add` | refused, `UnknownIssuer` | — |
| `tls_certs_only` of a key-only PEM: 0 certificates, and the client still builds | refused, `UnknownIssuer` | — |
| `tls_certs_only(test CA)`, server shows a test-CA leaf for `DNS:wrong.example` only | refused, `certificate not valid for name "127.0.0.1"` | — |

- `Certificate::from_pem_bundle` skips non-certificate blocks, and an empty result still builds a
  client that trusts nothing, so the resolver must refuse a bundle with no certificate.
  `RootCertStore::add` (what `tls_certs_only` uses) fails the whole build on one certificate
  webpki cannot parse; a platform bundle can hold such a certificate, so the resolver keeps only
  the ones `RootCertStore::add` accepts and refuses when none are left.
- The self-signed refusal said `CaUsedAsEndEntity` because `openssl req -x509` made a CA
  certificate. A test meaning "unknown issuer" must use an end-entity self-signed certificate, or
  the refusal proves a different rule.

## Crates for a TLS test front

`rustls` 0.23.36 and `tokio-rustls` 0.26.4 are already locked. `rcgen` is not; 0.14.10 is
current, and its default features are `crypto`, `pem` and `ring`, so it pulls no `aws-lc-rs`.
`tokio-rustls`' defaults include `aws_lc_rs`, so it needs `default-features = false, features =
["ring"]`. `cargo tree --locked -e normal,dev --target all -i aws-lc-sys` (and `openssl-sys`,
`native-tls`) reports no match today.

## What loopback cannot prove

That each release platform's bundle path exists and that AWS's real chain verifies against it,
under OpenSSL 3.0.8 in httpfs and webpki in the writer. A request to AWS's public endpoint with
the documented example key pair answers 403 or 404 once the handshake verifies, and a
certificate error otherwise. That is a statement about roots, not about verification being on:
with verification off it answers 403 too. The CI test legs (ubuntu-24.04, macos-15) and the
release legs (manylinux_2_28 containers, RHEL-family, on amd64 and arm64; macOS) have network.
