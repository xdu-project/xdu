//! Index locators that name an S3 prefix, and the one credential resolver every S3 path uses.
//!
//! An index argument is S3 only when its bytes begin with the lowercase `s3://` scheme. Every
//! other argument is a local path, exactly as before, including one that is not valid UTF-8:
//! classification reads the `OsStr` and never converts a local path to a `String`, since doing so
//! once broke non-UTF-8 index paths.
//!
//! Credentials, region and endpoint come from the `AWS_*` variables and the shared credentials
//! and config files, read here and nowhere else. The writer and the readers consume the same
//! `S3Settings`, so the two directions cannot resolve different identities or endpoints. Nothing
//! here reads instance metadata, SSO caches or a credential process, and no error message carries
//! a credential, an endpoint value or any other value read from those sources: each names the
//! variable or the file and profile it came from.
//!
//! The resolver also picks the one CA file both directions authenticate an `https` endpoint
//! against: the file `AWS_CA_BUNDLE` names, else the first platform bundle present. The writer's
//! client trusts only that file's certificates and the readers hand `httpfs` its path, so one
//! resolved endpoint gets one level of trust whichever way the bytes go.
//!
//! `S3Sink` is the crawler's S3 destination. Each chunk is one PUT at its final key, a
//! partition's stale tail goes by LIST and DELETE, and the completion marker is a DELETE before
//! the first chunk and a PUT after the last partition. Nothing it sends is staged on local disk.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use duckdb::types::Value;
use futures::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::client::{HttpClient, HttpConnector, HttpError, HttpErrorKind};
use object_store::path::{Path as ObjectPath, PathPart};
use object_store::{BackoffConfig, ClientOptions, ObjectStore, ObjectStoreExt, RetryConfig};
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::{PemObject, SectionKind};

use crate::COMPLETION_MARKER;
use crate::crawl::{IndexSink, TopEntry, chunk_final_name};

/// The scheme that marks an index argument as an S3 locator. Lowercase only: `S3://x` is a
/// relative local path, as it always was.
pub const S3_SCHEME: &str = "s3://";

/// Characters DuckDB's glob expansion treats as patterns. A prefix holding one would make the
/// index glob match keys outside the index, so the parser refuses them. A bucket cannot hold
/// one: the bucket name rules admit none.
const GLOB_METACHARACTERS: &[char] = &['*', '?', '[', ']', '{', '}'];

/// The shortest and longest bucket names S3 allows.
const BUCKET_NAME_LENGTH: std::ops::RangeInclusive<usize> = 3..=63;

/// Why `bucket` is not a name S3 gives a bucket, or `None` when it is one.
///
/// S3's rules are the allow-list: 3 to 63 characters, only lowercase letters, digits, `.` and
/// `-`, beginning and ending with a letter or digit, with no two `.` adjacent. The bucket
/// becomes part of the host every client addresses (`<bucket>.s3.<region>.amazonaws.com`) or
/// of the path under an endpoint override, so a character outside that set could move a
/// request to a host no setting named: `:` adds a port, `/`, `?` and `#` end the authority,
/// `@` turns what precedes it into user information, and `\`, `%` and whitespace are read as a
/// separator, an escape or an end by one URL parser or another. Legacy bucket names with
/// capitals or `_` are refused too; a virtual-hosted request, the only kind the writer sends
/// to AWS, cannot reach such a bucket.
///
/// The answer never quotes the bucket. It names a character only when that character cannot
/// belong to a credential: a `:` is named on its own, since in `key:secret@` it separates a
/// key from its secret, and it is checked before any other character is named.
fn bucket_name_problem(bucket: &str) -> Option<String> {
    if bucket.contains(':') {
        return Some(
            "contains ':', which would name a port or separate a key from its secret; an \
             endpoint's host and port come from AWS_ENDPOINT_URL_S3, AWS_ENDPOINT_URL or the \
             profile's endpoint_url"
                .to_string(),
        );
    }
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-';
    if let Some(c) = bucket.chars().find(|&c| !allowed(c)) {
        return Some(format!("contains {c:?}, which a bucket name cannot hold"));
    }
    if !BUCKET_NAME_LENGTH.contains(&bucket.len()) {
        return Some(format!("is {} character(s) long", bucket.len()));
    }
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !edge(bucket.chars().next()) || !edge(bucket.chars().last()) {
        return Some("begins or ends with '.' or '-'".to_string());
    }
    if bucket.contains("..") {
        return Some("holds two adjacent '.'".to_string());
    }
    None
}

/// The region AWS tools fall back to when no source names one.
pub const DEFAULT_REGION: &str = "us-east-1";

/// The profile used when `AWS_PROFILE` is unset or empty.
pub const DEFAULT_PROFILE: &str = "default";

/// Whether a raw index argument is an S3 locator, judged on its bytes alone.
///
/// This is the whole classification; it does not validate the rest of the locator.
pub fn is_s3_locator(raw: &OsStr) -> bool {
    raw.as_bytes().starts_with(S3_SCHEME.as_bytes())
}

/// Where an index lives: a local directory, or a prefix in an S3 bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexLocator {
    Local(PathBuf),
    S3(S3Locator),
}

impl IndexLocator {
    /// Classify a raw `-i`/`-o`/`XDU_INDEX` argument.
    ///
    /// A local argument is returned untouched, so a local path keeps every byte it was given.
    /// An S3 argument must parse, or the error says why.
    pub fn parse(raw: &OsStr) -> Result<Self> {
        if is_s3_locator(raw) {
            S3Locator::parse(raw).map(IndexLocator::S3)
        } else {
            Ok(IndexLocator::Local(PathBuf::from(raw)))
        }
    }
}

/// A bucket and a key prefix, without any credential or endpoint.
///
/// The prefix carries no leading or trailing `/`, so `<prefix>/<partition>/<chunk>.parquet`
/// is the key a chunk lives at; `s3://b/p/` and `s3://b/p` name the same index. An empty prefix
/// is the bucket's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Locator {
    bucket: String,
    prefix: String,
}

impl S3Locator {
    /// Parse an `s3://BUCKET[/PREFIX]` locator.
    ///
    /// Refused: a `user:secret@` form (credentials never travel in a locator), bytes that are
    /// not UTF-8, an empty bucket, a bucket S3 would not name, and glob metacharacters in the
    /// prefix. No refusal prints any part of the locator. Until the bucket is known to be a
    /// bucket name, whatever precedes the first `/` may hold user information, and a secret
    /// with a `/` in it spills past that into the prefix. So each error names its problem
    /// alone, and the `@` check runs first, on the bytes.
    pub fn parse(raw: &OsStr) -> Result<Self> {
        let Some(rest) = raw.as_bytes().strip_prefix(S3_SCHEME.as_bytes()) else {
            bail!("the argument is not an S3 locator; expected s3://BUCKET/PREFIX");
        };
        let authority = rest.split(|&b| b == b'/').next().unwrap_or_default();
        if authority.contains(&b'@') {
            bail!(
                "an S3 locator must not carry credentials before the bucket; give them through \
                 the AWS_* variables or the shared credentials file"
            );
        }
        let Some(rest) = raw.to_str().and_then(|text| text.strip_prefix(S3_SCHEME)) else {
            bail!("the S3 locator is not valid UTF-8");
        };
        let (bucket, prefix) = rest.split_once('/').unwrap_or((rest, ""));

        if bucket.is_empty() {
            bail!("the S3 locator names no bucket; expected s3://BUCKET/PREFIX");
        }
        if let Some(problem) = bucket_name_problem(bucket) {
            bail!(
                "the S3 locator's bucket {problem}; a bucket name has 3 to 63 characters, only \
                 lowercase letters, digits, '.' and '-', and begins and ends with a letter or \
                 digit"
            );
        }
        if let Some(c) = prefix.chars().find(|c| GLOB_METACHARACTERS.contains(c)) {
            bail!(
                "the S3 locator's prefix contains the glob metacharacter '{c}', which would let \
                 the index glob match keys outside the index"
            );
        }

        Ok(S3Locator {
            bucket: bucket.to_string(),
            prefix: prefix.trim_end_matches('/').to_string(),
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The `s3://` URL of `name` directly under the prefix: an object key, or a glob such as
    /// `*` or `**` when the result is handed to a glob.
    pub fn object_url(&self, name: &str) -> String {
        format!("{self}/{name}")
    }
}

impl fmt::Display for S3Locator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.prefix.is_empty() {
            write!(f, "{S3_SCHEME}{}", self.bucket)
        } else {
            write!(f, "{S3_SCHEME}{}/{}", self.bucket, self.prefix)
        }
    }
}

/// The environment the resolver reads: variables and files.
///
/// A trait so unit tests can resolve from fixture files without touching the process
/// environment, which the test harness shares across threads.
pub trait EnvSource {
    /// The variable's value, if set.
    fn var(&self, name: &str) -> Option<OsString>;

    /// The file's contents. `NotFound` means the file is absent, which is not an error.
    fn read_file(&self, file: &Path) -> io::Result<String> {
        fs::read_to_string(file)
    }
}

/// The running process's environment and filesystem.
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn var(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

/// An S3 endpoint override: scheme, host and port, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Endpoint {
    use_ssl: bool,
    authority: String,
}

impl S3Endpoint {
    /// Parse an `http://` or `https://` URL holding a host and an optional port.
    ///
    /// A path, query, fragment or user information is refused rather than dropped: the two
    /// clients would treat a path differently, and a URL carrying credentials is a leak
    /// waiting for a log line. The error is a reason only; the caller names the source.
    fn parse(raw: &str) -> std::result::Result<Self, &'static str> {
        let lower = raw.to_ascii_lowercase();
        let (use_ssl, rest) = if lower.starts_with("https://") {
            (true, &raw["https://".len()..])
        } else if lower.starts_with("http://") {
            (false, &raw["http://".len()..])
        } else {
            return Err("is not an http:// or https:// URL");
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(end);
        if !tail.is_empty() && tail != "/" {
            return Err("carries a path, query or fragment; give only scheme, host and port");
        }
        if authority.contains('@') {
            return Err("carries user information; give credentials through AWS_* instead");
        }
        let host_end = if authority.starts_with('[') {
            match authority.find(']') {
                Some(i) => i + 1,
                None => return Err("has an unterminated IPv6 address"),
            }
        } else {
            authority.find(':').unwrap_or(authority.len())
        };
        let (host, port) = authority.split_at(host_end);
        if host.is_empty() || host == "[]" {
            return Err("names no host");
        }
        // Only characters every URL parser reads alike. A `\`, `%` or whitespace in a host is a
        // separator, an escape or an end to one parser and part of the name to another, so the
        // writer and the readers could reach different hosts from the one value.
        let host_holds_only_host_characters = match host.strip_prefix('[') {
            Some(v6) => v6
                .trim_end_matches(']')
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.'),
            None => host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_'),
        };
        if !host_holds_only_host_characters {
            return Err(
                "has a host holding a character other than letters, digits, '.', '-' and '_' \
                 (or, in brackets, an IPv6 address)",
            );
        }
        if let Some(port) = port.strip_prefix(':') {
            if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
                return Err("has a port that is not a number");
            }
        } else if !port.is_empty() {
            return Err("has text after the host that is not a port");
        }
        Ok(S3Endpoint {
            use_ssl,
            authority: authority.to_string(),
        })
    }

    /// `host[:port]`, the form DuckDB's `ENDPOINT` takes.
    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub fn use_ssl(&self) -> bool {
        self.use_ssl
    }

    /// The endpoint as a URL, the form an HTTP client takes.
    pub fn url(&self) -> String {
        let scheme = if self.use_ssl { "https" } else { "http" };
        format!("{scheme}://{}", self.authority)
    }
}

/// The platform CA bundles, in the order the pinned `httpfs` probes them: Debian-family hosts,
/// RHEL-family hosts (two spellings), SUSE, then macOS and the BSDs.
pub const PLATFORM_CA_BUNDLES: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
];

/// Where a trust source came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustOrigin {
    /// The file `AWS_CA_BUNDLE` names.
    AwsCaBundle,
    /// The first of `PLATFORM_CA_BUNDLES` present.
    Platform,
}

/// The one CA file every `https` connection authenticates its server against, in both
/// directions.
///
/// It is read once, by the resolver. The writer trusts the certificates read here and nothing
/// else; the readers bind the path for `httpfs`, whose OpenSSL reopens it at the first request.
/// The certificates kept are those both TLS stacks load, and they include a self-signed root,
/// which the readers need a chain to end at, so the two directions trust one set.
/// `SSL_CERT_FILE`, `SSL_CERT_DIR`, OpenSSL's compiled-in directory and the macOS keychain are
/// never a source.
#[derive(Clone)]
pub struct TrustSource {
    /// Absolute, and valid UTF-8, since the readers bind it as text.
    path: String,
    origin: TrustOrigin,
    certificates: Vec<CertificateDer<'static>>,
}

impl fmt::Debug for TrustSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrustSource")
            .field("path", &self.path)
            .field("origin", &self.origin)
            .field("certificates", &self.certificates.len())
            .finish()
    }
}

impl TrustSource {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn origin(&self) -> TrustOrigin {
        self.origin
    }

    /// The CA certificates kept from the file, in file order.
    pub fn certificates(&self) -> &[CertificateDer<'static>] {
        &self.certificates
    }
}

/// Everything an S3 client needs, resolved once from the AWS variables and shared files.
///
/// `Debug` prints neither the secret nor the session token.
#[derive(Clone)]
pub struct S3Settings {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub region: String,
    /// The override the resolver's sources name; `None` is AWS's regional endpoint, which
    /// `endpoint_authority` spells out.
    pub endpoint: Option<S3Endpoint>,
    /// Path-style addressing (`host/bucket/key`). On whenever an endpoint override is set,
    /// since S3-compatible stores rarely serve virtual-hosted names; off for AWS itself.
    pub path_style: bool,
    /// The CA file the server is authenticated against: `Some` exactly when requests go over
    /// TLS, and `None` for a plain `http://` override, where TLS does not apply.
    pub trust: Option<TrustSource>,
}

impl fmt::Debug for S3Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Settings")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .field("region", &self.region)
            .field("endpoint", &self.endpoint)
            .field("path_style", &self.path_style)
            .field("trust", &self.trust)
            .finish()
    }
}

/// The name of the one secret a reader connection holds.
const DUCKDB_SECRET_NAME: &str = "xdu_s3";

/// The statements that make `httpfs` verify every server, each whole on its own line: the
/// client pinned to httplib, then verification on for both of its clients.
const DUCKDB_TLS_PINNING: [&str; 3] = [
    "SET httpfs_client_implementation = 'httplib'",
    "SET enable_server_cert_verification = true",
    "SET enable_curl_server_cert_verification = true",
];

/// The statement that names the trust source's file to `httpfs`, its path bound.
const DUCKDB_TLS_CA_FILE: &str = "SET ca_cert_file = ?";

impl S3Settings {
    /// Resolve credentials, region and endpoint.
    ///
    /// Credentials: `AWS_ACCESS_KEY_ID` with `AWS_SECRET_ACCESS_KEY` (and `AWS_SESSION_TOKEN`
    /// if set), else the profile in the shared credentials file, else the same profile in the
    /// config file. The profile is `AWS_PROFILE`, else `default`. Region: `AWS_REGION`, then
    /// `AWS_DEFAULT_REGION`, then the profile's `region`, then `us-east-1`. Endpoint:
    /// `AWS_ENDPOINT_URL_S3`, then `AWS_ENDPOINT_URL`, then the profile's `services` section
    /// (`s3 = endpoint_url = …`), then its `endpoint_url`. The files are
    /// `AWS_SHARED_CREDENTIALS_FILE` and `AWS_CONFIG_FILE`, else `~/.aws/credentials` and
    /// `~/.aws/config`.
    ///
    /// Trust, for an `https` endpoint (every AWS one): the CA file `AWS_CA_BUNDLE` names, else
    /// the first of `PLATFORM_CA_BUNDLES` present. A plain `http://` override resolves none, so
    /// an unusable `AWS_CA_BUNDLE` does not stop it.
    ///
    /// It fails closed: half-set environment credentials, a profile holding only one half of
    /// a key pair, a profile named by `AWS_PROFILE` that neither file holds, no credentials at
    /// all, an endpoint that is not a bare `http(s)://host[:port]`, and, for an `https`
    /// endpoint, no trust source or one that is unreadable, holds no usable CA certificate or
    /// holds no self-signed root are errors.
    pub fn resolve(env: &dyn EnvSource) -> Result<S3Settings> {
        let profile_var = env_text(env, "AWS_PROFILE")?;
        let profile = profile_var
            .clone()
            .unwrap_or_else(|| DEFAULT_PROFILE.to_string());

        let credentials_file = shared_file(env, "AWS_SHARED_CREDENTIALS_FILE", "credentials")?;
        let config_file = shared_file(env, "AWS_CONFIG_FILE", "config")?;
        let credentials = load_ini(env, credentials_file.as_deref())?;
        let config = load_ini(env, config_file.as_deref())?;

        let credentials_section = credentials.section(&profile);
        let config_section = config_profile(&config, &profile);
        if profile_var.is_some() && credentials_section.is_none() && config_section.is_none() {
            bail!(
                "AWS_PROFILE names profile '{}', which neither {} nor {} defines",
                profile,
                describe_file(credentials_file.as_deref()),
                describe_file(config_file.as_deref())
            );
        }

        let (access_key_id, secret_access_key, session_token) = match env_credentials(env)? {
            Some(found) => found,
            None => file_credentials(credentials_section, credentials_file.as_deref(), &profile)?
                .or(file_credentials(
                    config_section,
                    config_file.as_deref(),
                    &profile,
                )?)
                .ok_or_else(|| {
                    anyhow!(
                        "no S3 credentials: AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY are \
                         unset, and profile '{}' holds no aws_access_key_id and \
                         aws_secret_access_key in {} or {}",
                        profile,
                        describe_file(credentials_file.as_deref()),
                        describe_file(config_file.as_deref())
                    )
                })?,
        };

        let (region, region_source) = match env_text(env, "AWS_REGION")? {
            Some(region) => (region, "AWS_REGION".to_string()),
            None => match env_text(env, "AWS_DEFAULT_REGION")? {
                Some(region) => (region, "AWS_DEFAULT_REGION".to_string()),
                None => match config_section.and_then(|s| s.value("region")) {
                    Some(region) => (
                        region.to_string(),
                        format!(
                            "the region of profile '{}' in {}",
                            profile,
                            describe_file(config_file.as_deref())
                        ),
                    ),
                    None => (DEFAULT_REGION.to_string(), "the default region".to_string()),
                },
            },
        };

        let endpoint = resolve_endpoint(
            env,
            &config,
            config_section,
            config_file.as_deref(),
            &profile,
        )?;
        // With no override the region names the host both clients address, so it must be a
        // region name and nothing that could carry a host or a path into the URL.
        if endpoint.is_none() && !is_aws_region_name(&region) {
            bail!(
                "{region_source} is not an AWS region name (lowercase letters, digits and '-'), \
                 and with no endpoint override the region names the S3 host; fix it, or name \
                 an S3-compatible endpoint with AWS_ENDPOINT_URL_S3"
            );
        }
        let path_style = endpoint.is_some();
        let trust = if endpoint.as_ref().is_none_or(S3Endpoint::use_ssl) {
            Some(resolve_trust(env)?)
        } else {
            None
        };

        Ok(S3Settings {
            access_key_id,
            secret_access_key,
            session_token,
            region,
            endpoint,
            path_style,
            trust,
        })
    }

    /// The `host[:port]` every client addresses: the override's, or AWS's regional S3
    /// endpoint, `s3.<region>.amazonaws.com`.
    pub fn endpoint_authority(&self) -> String {
        match &self.endpoint {
            Some(endpoint) => endpoint.authority().to_string(),
            None => format!("s3.{}.amazonaws.com", self.region),
        }
    }

    /// Whether requests go over TLS: always for AWS, as the override's scheme says otherwise.
    pub fn use_ssl(&self) -> bool {
        self.endpoint.as_ref().is_none_or(S3Endpoint::use_ssl)
    }

    /// The endpoint URL the writer's client takes for `bucket`: the host itself for path-style
    /// addressing, else the bucket's virtual-hosted name under it. Built from
    /// `endpoint_authority`, as the readers' `ENDPOINT` is, so both reach the same host.
    pub fn client_endpoint_url(&self, bucket: &str) -> String {
        let scheme = if self.use_ssl() { "https" } else { "http" };
        let authority = self.endpoint_authority();
        if self.path_style {
            format!("{scheme}://{authority}")
        } else {
            format!("{scheme}://{bucket}.{authority}")
        }
    }

    /// The `CREATE SECRET` a reader connection runs, and the values it binds.
    ///
    /// Every value is a bound parameter, and the text is fixed. The secret names every key
    /// `httpfs` would otherwise take from a setting, because for a key the secret lacks it falls
    /// back to the `s3_*` setting of that name, which it fills from the environment when it
    /// loads: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, `AWS_REGION`
    /// and `AWS_DEFAULT_REGION`, `DUCKDB_S3_ENDPOINT`, `DUCKDB_S3_USE_SSL`,
    /// `DUCKDB_S3_REQUESTER_PAYS` and `DUCKDB_S3_KMS_KEY_ID`. So `ENDPOINT` is bound even for
    /// AWS, a missing session token is bound as empty (no token header is sent), requester-pays
    /// is off as the writer's is, and the KMS key is empty (it would only mark writes, which a
    /// reader never makes). `URL_STYLE` and `URL_COMPATIBILITY_MODE` have no environment source.
    /// Held this way the secret shows as `redacted` in `duckdb_secrets()` and appears in no
    /// `duckdb_settings()` row.
    pub fn duckdb_secret_statement(&self) -> (String, Vec<Value>) {
        let sql = format!(
            "CREATE TEMPORARY SECRET {DUCKDB_SECRET_NAME} (TYPE s3, PROVIDER config, \
             KEY_ID ?, SECRET ?, SESSION_TOKEN ?, REGION ?, ENDPOINT ?, URL_STYLE ?, USE_SSL ?, \
             REQUESTER_PAYS ?, KMS_KEY_ID ?)"
        );
        let url_style = if self.path_style { "path" } else { "vhost" };
        let params = vec![
            Value::Text(self.access_key_id.clone()),
            Value::Text(self.secret_access_key.clone()),
            Value::Text(self.session_token.clone().unwrap_or_default()),
            Value::Text(self.region.clone()),
            Value::Text(self.endpoint_authority()),
            Value::Text(url_style.to_string()),
            Value::Boolean(self.use_ssl()),
            Value::Boolean(false),
            Value::Text(String::new()),
        ];
        (sql, params)
    }

    /// The statements a reader connection runs after the `LOAD` and before the `CREATE
    /// SECRET`, each with the values it binds, so that `httpfs` authenticates every `https`
    /// server against the trust source.
    ///
    /// `httpfs` verifies nothing until told to, and with no CA file it trusts OpenSSL's
    /// defaults, `SSL_CERT_FILE` among them. So its client is pinned to httplib, the one that
    /// reads the CA file: the curl client ignores it, an engine whose default moved to curl
    /// would change the trust source silently, and an engine without httplib fails the
    /// statement. Verification goes on for both clients, so curl, if anything selected it,
    /// would still refuse. Then, when there is a trust source, its path is bound as the CA
    /// file. The text of every statement is fixed; the path is a bound value, never SQL text.
    pub fn duckdb_tls_statements(&self) -> Vec<(&'static str, Vec<Value>)> {
        let mut statements: Vec<(&'static str, Vec<Value>)> = DUCKDB_TLS_PINNING
            .iter()
            .map(|sql| (*sql, Vec::new()))
            .collect();
        if let Some(trust) = &self.trust {
            statements.push((DUCKDB_TLS_CA_FILE, vec![Value::Text(trust.path.clone())]));
        }
        statements
    }
}

/// Pick the trust source: the file `AWS_CA_BUNDLE` names when it is set and non-empty, else the
/// first of `PLATFORM_CA_BUNDLES` present. A candidate that is present but unreadable or
/// unusable is an error; the search does not skip on past it. Every error names
/// `AWS_CA_BUNDLE`, the remedy, and never says the server's certificate was refused: that
/// phrase belongs to the endpoint's refusal alone.
fn resolve_trust(env: &dyn EnvSource) -> Result<TrustSource> {
    if let Some(named) = env_text(env, "AWS_CA_BUNDLE")? {
        let path = std::path::absolute(&named)
            .ok()
            .and_then(|absolute| absolute.into_os_string().into_string().ok())
            .ok_or_else(|| {
                anyhow!("AWS_CA_BUNDLE names {named}, which cannot be made an absolute path")
            })?;
        let text = env.read_file(Path::new(&path)).map_err(|failure| {
            anyhow!(
                "AWS_CA_BUNDLE names {path}, which cannot be read ({})",
                failure.kind()
            )
        })?;
        let certificates = bundle_certificates(&text)
            .map_err(|reason| anyhow!("AWS_CA_BUNDLE names {path}, which {reason}"))?;
        return Ok(TrustSource {
            path,
            origin: TrustOrigin::AwsCaBundle,
            certificates,
        });
    }
    for candidate in PLATFORM_CA_BUNDLES {
        let text = match env.read_file(Path::new(candidate)) {
            Ok(text) => text,
            Err(failure) if failure.kind() == io::ErrorKind::NotFound => continue,
            Err(failure) => bail!(
                "the platform CA bundle {candidate} cannot be read ({}); set AWS_CA_BUNDLE to a \
                 CA file",
                failure.kind()
            ),
        };
        let certificates = bundle_certificates(&text).map_err(|reason| {
            anyhow!("the platform CA bundle {candidate} {reason}; set AWS_CA_BUNDLE to a CA file")
        })?;
        return Ok(TrustSource {
            path: candidate.to_string(),
            origin: TrustOrigin::Platform,
            certificates,
        });
    }
    bail!(
        "no platform CA bundle found (looked for {}); set AWS_CA_BUNDLE to a CA file",
        PLATFORM_CA_BUNDLES.join(", ")
    )
}

/// The CA certificates of a PEM bundle that both TLS stacks load, or why the bundle cannot
/// serve, worded to follow the file's name.
///
/// The readers' OpenSSL fails at its first request on a block that is not valid PEM or a
/// certificate that is not valid DER, and it trusts a `TRUSTED CERTIFICATE` block, which the
/// writer's webpki cannot read; each of those refuses the whole file here, so the two stacks
/// never load different sets. A certificate that is valid DER but that webpki will not take as
/// an anchor is dropped: kept, it would fail the writer's client, and a platform bundle can
/// hold one. Blocks of other kinds, such as keys, are skipped.
///
/// The certificates kept must include a self-signed root. webpki anchors at any certificate in
/// the file, but the readers' OpenSSL accepts a chain only when it ends at a self-signed
/// certificate there, so a file without one, such as an intermediate CA alone, would let the
/// writer build an index the readers then refuse.
fn bundle_certificates(text: &str) -> std::result::Result<Vec<CertificateDer<'static>>, String> {
    if text.lines().any(|line| {
        line.trim_start()
            .starts_with("-----BEGIN TRUSTED CERTIFICATE-----")
    }) {
        return Err(
            "holds a TRUSTED CERTIFICATE block, which only one of the two TLS stacks \
                    reads; give its plain CERTIFICATE form"
                .to_string(),
        );
    }
    let mut anchors = rustls::RootCertStore::empty();
    let mut kept = Vec::new();
    for section in <(SectionKind, Vec<u8>)>::pem_slice_iter(text.as_bytes()) {
        let Ok((kind, der)) = section else {
            return Err("is not valid PEM".to_string());
        };
        if kind != SectionKind::Certificate {
            continue;
        }
        if !is_certificate_der(&der) {
            return Err("holds a CERTIFICATE block that is not a DER certificate".to_string());
        }
        let certificate = CertificateDer::from(der);
        if anchors.add(certificate.clone()).is_ok() {
            kept.push(certificate);
        }
    }
    if kept.is_empty() {
        return Err("holds no usable CA certificate".to_string());
    }
    if !kept
        .iter()
        .any(|certificate| is_self_signed_root(certificate))
    {
        return Err(
            "holds no root CA certificate (one issued and signed by itself), so no chain can end \
             in it; add the root its CA certificates chain to"
                .to_string(),
        );
    }
    Ok(kept)
}

/// Whether `der` is a self-signed root: its issuer is its own subject, byte for byte, and its
/// signature verifies under its own public key with an algorithm the writer's TLS stack
/// verifies.
///
/// That is what OpenSSL needs at the end of a chain: it takes a certificate as self-signed on a
/// name match, a key identifier naming its own key and a signature algorithm of its key's
/// family, and both conditions here imply those for any certificate a CA issues as a root. A
/// root whose self-signature uses an algorithm the writer does not verify, such as SHA-1 RSA,
/// does not count, so a file holding only such roots is refused: it fails closed.
fn is_self_signed_root(der: &[u8]) -> bool {
    let Some(parts) = CertificateParts::parse(der) else {
        return false;
    };
    parts.issuer == parts.subject
        && rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .all
            .iter()
            .filter(|algorithm| {
                algorithm.signature_alg_id().as_ref() == parts.signature_algorithm
                    && algorithm.public_key_alg_id().as_ref() == parts.key_algorithm
            })
            .any(|algorithm| {
                algorithm
                    .verify_signature(parts.public_key, parts.signed, parts.signature)
                    .is_ok()
            })
}

/// The parts of a DER certificate a signature check needs, each as webpki reads it: the
/// algorithm identifiers without their SEQUENCE header and the bit strings without their
/// unused-bits octet.
struct CertificateParts<'a> {
    /// The whole `tbsCertificate` element, the bytes the signature covers.
    signed: &'a [u8],
    signature_algorithm: &'a [u8],
    signature: &'a [u8],
    issuer: &'a [u8],
    subject: &'a [u8],
    key_algorithm: &'a [u8],
    public_key: &'a [u8],
}

impl<'a> CertificateParts<'a> {
    fn parse(der: &'a [u8]) -> Option<CertificateParts<'a>> {
        const INTEGER: u8 = 0x02;
        const BIT_STRING: u8 = 0x03;
        const SEQUENCE: u8 = 0x30;
        const VERSION: u8 = 0xa0;
        let Some((SEQUENCE, certificate, [])) = der_element(der) else {
            return None;
        };
        let Some((SEQUENCE, tbs, rest)) = der_element(certificate) else {
            return None;
        };
        let signed = &certificate[..certificate.len() - rest.len()];
        let Some((SEQUENCE, signature_algorithm, rest)) = der_element(rest) else {
            return None;
        };
        let Some((BIT_STRING, [0, signature @ ..], [])) = der_element(rest) else {
            return None;
        };

        // The next field of `fields`, which must carry tag `want`.
        fn take<'b>(fields: &mut &'b [u8], want: u8) -> Option<&'b [u8]> {
            let (tag, content, rest) = der_element(fields)?;
            if tag != want {
                return None;
            }
            *fields = rest;
            Some(content)
        }
        let mut fields = tbs;
        if fields.first() == Some(&VERSION) {
            take(&mut fields, VERSION)?;
        }
        take(&mut fields, INTEGER)?;
        take(&mut fields, SEQUENCE)?;
        let issuer = take(&mut fields, SEQUENCE)?;
        take(&mut fields, SEQUENCE)?;
        let subject = take(&mut fields, SEQUENCE)?;
        let key_info = take(&mut fields, SEQUENCE)?;
        let Some((SEQUENCE, key_algorithm, rest)) = der_element(key_info) else {
            return None;
        };
        let Some((BIT_STRING, [0, public_key @ ..], [])) = der_element(rest) else {
            return None;
        };
        Some(CertificateParts {
            signed,
            signature_algorithm,
            signature,
            issuer,
            subject,
            key_algorithm,
            public_key,
        })
    }
}

/// Whether `der` has a DER certificate's shape: one SEQUENCE spanning the whole input, holding
/// a SEQUENCE, a SEQUENCE and a BIT STRING (the signed body, the signature algorithm and the
/// signature), with every element below it well formed.
fn is_certificate_der(der: &[u8]) -> bool {
    const SEQUENCE: u8 = 0x30;
    const BIT_STRING: u8 = 0x03;
    let Some((SEQUENCE, body, [])) = der_element(der) else {
        return false;
    };
    let mut tags = Vec::new();
    let mut rest = body;
    while let Some((tag, content, after)) = der_element(rest) {
        if is_constructed(tag) && !der_well_formed(content) {
            return false;
        }
        tags.push(tag);
        rest = after;
    }
    rest.is_empty() && tags == [SEQUENCE, SEQUENCE, BIT_STRING]
}

fn is_constructed(tag: u8) -> bool {
    tag & 0x20 != 0
}

/// Every element of `input` is a well-formed DER element, through every constructed one.
fn der_well_formed(mut input: &[u8]) -> bool {
    while !input.is_empty() {
        let Some((tag, content, after)) = der_element(input) else {
            return false;
        };
        if is_constructed(tag) && !der_well_formed(content) {
            return false;
        }
        input = after;
    }
    true
}

/// The first DER element of `input`: its tag, its content and what follows it. Only one-byte
/// tags, which are all an X.509 certificate uses, and definite lengths in their shortest form,
/// as DER requires.
fn der_element(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    if tag & 0x1f == 0x1f {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (digits, rest) = rest.split_at(count);
        if digits[0] == 0 {
            return None;
        }
        let len = digits
            .iter()
            .fold(0usize, |len, digit| (len << 8) | usize::from(*digit));
        if len < 0x80 {
            return None;
        }
        (len, rest)
    };
    if rest.len() < len {
        return None;
    }
    let (content, rest) = rest.split_at(len);
    Some((tag, content, rest))
}

/// An AWS region name: lowercase letters, digits and `-`, as `us-east-1` or `eu-central-2`.
fn is_aws_region_name(region: &str) -> bool {
    !region.is_empty()
        && !region.starts_with('-')
        && region
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A variable's value as text, treating set-but-empty as unset.
fn env_text(env: &dyn EnvSource, name: &str) -> Result<Option<String>> {
    match env.var(name) {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => value
            .into_string()
            .map(Some)
            .map_err(|_| anyhow!("{name} is not valid UTF-8")),
    }
}

fn env_credentials(env: &dyn EnvSource) -> Result<Option<(String, String, Option<String>)>> {
    let id = env_text(env, "AWS_ACCESS_KEY_ID")?;
    let secret = env_text(env, "AWS_SECRET_ACCESS_KEY")?;
    let token = env_text(env, "AWS_SESSION_TOKEN")?;
    match (id, secret) {
        (Some(id), Some(secret)) => Ok(Some((id, secret, token))),
        (Some(_), None) => {
            bail!("AWS_ACCESS_KEY_ID is set but AWS_SECRET_ACCESS_KEY is not; set both or neither")
        }
        (None, Some(_)) => {
            bail!("AWS_SECRET_ACCESS_KEY is set but AWS_ACCESS_KEY_ID is not; set both or neither")
        }
        (None, None) if token.is_some() => {
            bail!("AWS_SESSION_TOKEN is set without AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY")
        }
        (None, None) => Ok(None),
    }
}

fn file_credentials(
    section: Option<&IniSection>,
    file: Option<&Path>,
    profile: &str,
) -> Result<Option<(String, String, Option<String>)>> {
    let Some(section) = section else {
        return Ok(None);
    };
    let id = section.value("aws_access_key_id");
    let secret = section.value("aws_secret_access_key");
    let token = section.value("aws_session_token").map(str::to_string);
    match (id, secret) {
        (Some(id), Some(secret)) => Ok(Some((id.to_string(), secret.to_string(), token))),
        (None, None) => Ok(None),
        _ => bail!(
            "profile '{}' in {} holds only one of aws_access_key_id and aws_secret_access_key",
            profile,
            describe_file(file)
        ),
    }
}

fn resolve_endpoint(
    env: &dyn EnvSource,
    config: &Ini,
    config_section: Option<&IniSection>,
    config_file: Option<&Path>,
    profile: &str,
) -> Result<Option<S3Endpoint>> {
    for name in ["AWS_ENDPOINT_URL_S3", "AWS_ENDPOINT_URL"] {
        if let Some(raw) = env_text(env, name)? {
            return S3Endpoint::parse(&raw)
                .map(Some)
                .map_err(|reason| anyhow!("{name} {reason}"));
        }
    }
    let Some(section) = config_section else {
        return Ok(None);
    };
    if let Some(services) = section.value("services") {
        let from_services = config
            .section(&format!("services {services}"))
            .and_then(|s| s.nested("s3", "endpoint_url"));
        if let Some(raw) = from_services {
            return S3Endpoint::parse(raw).map(Some).map_err(|reason| {
                anyhow!(
                    "the s3 endpoint_url of services '{}' in {} {}",
                    services,
                    describe_file(config_file),
                    reason
                )
            });
        }
    }
    match section.value("endpoint_url") {
        Some(raw) => S3Endpoint::parse(raw).map(Some).map_err(|reason| {
            anyhow!(
                "the endpoint_url of profile '{}' in {} {}",
                profile,
                describe_file(config_file),
                reason
            )
        }),
        None => Ok(None),
    }
}

/// The path of a shared file: the variable if set, else `~/.aws/<name>`, else none.
fn shared_file(env: &dyn EnvSource, var: &str, name: &str) -> Result<Option<PathBuf>> {
    if let Some(value) = env.var(var).filter(|v| !v.is_empty()) {
        return Ok(Some(PathBuf::from(value)));
    }
    Ok(env
        .var("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| Path::new(&home).join(".aws").join(name)))
}

fn describe_file(file: Option<&Path>) -> String {
    match file {
        Some(file) => file.display().to_string(),
        None => "no file (HOME is unset)".to_string(),
    }
}

/// An absent file is an empty one, as it is to every AWS tool; any other read failure is an
/// error naming the file.
fn load_ini(env: &dyn EnvSource, file: Option<&Path>) -> Result<Ini> {
    let Some(file) = file else {
        return Ok(Ini::default());
    };
    match env.read_file(file) {
        Ok(text) => Ok(Ini::parse(&text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Ini::default()),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", file.display())),
    }
}

/// A profile's section in the config file: `[profile NAME]`, or `[default]` for the default.
fn config_profile<'a>(config: &'a Ini, profile: &str) -> Option<&'a IniSection> {
    config.section(&format!("profile {profile}")).or_else(|| {
        if profile == DEFAULT_PROFILE {
            config.section(DEFAULT_PROFILE)
        } else {
            None
        }
    })
}

/// The subset of the AWS shared-file INI dialect the resolver needs.
///
/// Sections are `[name]`, optionally followed by a `#` or `;` comment, with whitespace inside
/// the brackets not part of the name; names are case-sensitive. A line opening with `[` that is
/// not such a header starts a section no lookup can name, so the keys under it reach no
/// profile. Keys are case-folded and values trimmed; whole-line `#` and `;` comments are
/// skipped. A `#` or `;` after a value is part of the value, as Python's `configparser` (and
/// so the AWS CLI) reads it. A key with an empty value opens a nested block whose indented
/// `key = value` lines belong to it, the form a `services` section uses for `s3 =` followed
/// by `  endpoint_url = …`. Generic INI crates flatten that block, which is why this is
/// hand-rolled.
#[derive(Debug, Default)]
struct Ini {
    sections: HashMap<String, IniSection>,
}

#[derive(Debug, Default)]
struct IniSection {
    values: HashMap<String, String>,
    nested: HashMap<String, HashMap<String, String>>,
}

impl IniSection {
    fn value(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn nested(&self, block: &str, key: &str) -> Option<&str> {
        self.nested
            .get(block)
            .and_then(|b| b.get(key))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
}

impl Ini {
    fn parse(text: &str) -> Ini {
        let mut ini = Ini::default();
        let mut current: Option<String> = None;
        let mut open_block: Option<String> = None;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            if let Some(inner) = trimmed.strip_prefix('[') {
                // Every line opening with `[` ends the section above, so a header this parser
                // cannot read never hands its keys to the previous profile.
                current = section_name(inner);
                if let Some(name) = &current {
                    ini.sections.entry(name.clone()).or_default();
                }
                open_block = None;
                continue;
            }
            let (Some(section), Some((key, value))) = (current.as_ref(), trimmed.split_once('='))
            else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            let section = ini.sections.entry(section.clone()).or_default();
            let indented = line.starts_with([' ', '\t']);
            match (&open_block, indented) {
                (Some(block), true) => {
                    section
                        .nested
                        .entry(block.clone())
                        .or_default()
                        .insert(key, value);
                }
                _ => {
                    open_block = value.is_empty().then(|| key.clone());
                    section.values.insert(key, value);
                }
            }
        }
        ini
    }

    fn section(&self, name: &str) -> Option<&IniSection> {
        self.sections.get(name)
    }
}

/// The name a section header gives, from the text after its `[`: what lies before the first
/// `]`, with runs of whitespace collapsed to one space and none at either end. After the `]`
/// only a `#` or `;` comment may follow. Anything else is no header, so `None`.
fn section_name(inner: &str) -> Option<String> {
    let (name, tail) = inner.split_once(']')?;
    let tail = tail.trim_start();
    if !(tail.is_empty() || tail.starts_with(['#', ';'])) {
        return None;
    }
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(name)
}

/// Why a top-level source directory cannot be a partition of an S3 index, or `None` when it
/// can.
///
/// A partition's name becomes a key segment the writer PUTs under and a glob segment the
/// readers match, so it must reach both byte for byte. Refused: a name that is not UTF-8 (it
/// would be stored lossily); a DuckDB glob metacharacter, which would make the readers' glob
/// match other partitions; and anything the S3 client percent-encodes in a key segment, since
/// the stored key would then differ from the directory's name: a non-ASCII character, a control
/// character, one of the characters AWS advises against in keys (`\ { } ^ % ` ] " > [ ~ < # |`),
/// or a name that is exactly `.` or `..`.
pub fn s3_partition_name_problem(name: &OsStr) -> Option<&'static str> {
    let Some(name) = name.to_str() else {
        return Some("is not valid UTF-8");
    };
    if name.contains(GLOB_METACHARACTERS) {
        return Some("holds a glob metacharacter, which the readers' index glob would expand");
    }
    if !survives_as_key_segment(name) {
        return Some(
            "holds a character the S3 client percent-encodes in a key (non-ASCII, a control \
             character, or one of \\ { } ^ % ` ] \" > [ ~ < # |), or is . or .., so the stored \
             key would differ from it",
        );
    }
    None
}

/// Whether `segment` is stored under exactly this text by the S3 client.
fn survives_as_key_segment(segment: &str) -> bool {
    !segment.is_empty() && PathPart::from(segment).as_ref() == segment
}

/// Refuse the run when any top-level source directory cannot be a partition of an S3 index.
///
/// Every directory is checked, whether or not a `--partition` filter selects it, as the
/// reserved-name check does: the refusal must come before the completion marker is cleared,
/// and it names the first offending directory.
pub fn check_s3_partition_names(entries: &[TopEntry], top_dir: &Path) -> Result<()> {
    for entry in entries.iter().filter(|e| e.is_dir) {
        let name = entry.path.file_name().unwrap_or(entry.path.as_os_str());
        if let Some(problem) = s3_partition_name_problem(name) {
            bail!(
                "top-level directory '{}' in {} cannot be a partition of an S3 index: its name {}; \
                 rename it before indexing into S3",
                name.to_string_lossy(),
                top_dir.display(),
                problem
            );
        }
    }
    Ok(())
}

/// How long the writer waits to open a connection to the endpoint.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long one request may take, a chunk PUT included.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Retries of a failed request: few and short, so an unreachable endpoint fails the run in
/// seconds rather than the minutes the client would otherwise spend.
fn retry_config() -> RetryConfig {
    RetryConfig {
        backoff: BackoffConfig {
            init_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(2),
            base: 2.0,
        },
        max_retries: 3,
        retry_timeout: Duration::from_secs(30),
    }
}

/// The writer's HTTP client: direct to the endpoint, whatever the proxy variables say, as the
/// readers' engine is; with its own timeouts, since a custom connector bypasses the store's.
///
/// It trusts the trust source's certificates and nothing else: not the platform's verifier,
/// not its native store, and not `SSL_CERT_FILE`. For a plain `http://` endpoint there is no
/// source and the list is empty, so no TLS handshake can complete, a redirect's included.
#[derive(Debug)]
struct DirectConnector {
    roots: Vec<CertificateDer<'static>>,
}

impl HttpConnector for DirectConnector {
    fn connect(&self, _options: &ClientOptions) -> object_store::Result<HttpClient> {
        let generic = |source: reqwest::Error| object_store::Error::Generic {
            store: "S3",
            source: Box::new(source),
        };
        let roots = self
            .roots
            .iter()
            .map(|der| reqwest::Certificate::from_der(der))
            .collect::<reqwest::Result<Vec<_>>>()
            .map_err(generic)?;
        let client = reqwest::Client::builder()
            .tls_certs_only(roots)
            .no_proxy()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(generic)?;
        Ok(HttpClient::new(client))
    }
}

/// Use ring for every TLS connection this process makes. Installing it twice is harmless;
/// building the client without it would abort the process.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// An index under an S3 prefix, as the crawler's destination.
///
/// The sink owns a two-worker tokio runtime. Driver threads, which are not runtime threads,
/// block on it for each request; the rayon pool never touches it.
pub struct S3Sink {
    store: Arc<dyn ObjectStore>,
    runtime: tokio::runtime::Runtime,
    locator: S3Locator,
    /// The secret and session token, struck from any error text as a last guard.
    redact: Vec<String>,
}

impl S3Sink {
    /// A sink for `locator`, with the client built from `settings` alone: the static key pair,
    /// region and endpoint the resolver found, the endpoint always given explicitly, never the
    /// client's own environment or metadata lookups. Nothing is sent until `preflight`.
    pub fn connect(locator: &S3Locator, settings: &S3Settings) -> Result<S3Sink> {
        install_crypto_provider();
        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(locator.bucket())
            .with_region(&settings.region)
            .with_access_key_id(&settings.access_key_id)
            .with_secret_access_key(&settings.secret_access_key)
            .with_virtual_hosted_style_request(!settings.path_style)
            .with_endpoint(settings.client_endpoint_url(locator.bucket()))
            .with_allow_http(!settings.use_ssl())
            .with_retry(retry_config())
            .with_disable_bulk_delete(true)
            .with_http_connector(DirectConnector {
                roots: settings
                    .trust
                    .as_ref()
                    .map(|trust| trust.certificates.clone())
                    .unwrap_or_default(),
            });
        if let Some(token) = &settings.session_token {
            builder = builder.with_token(token);
        }
        let store = builder
            .build()
            .with_context(|| format!("cannot configure an S3 client for {locator}"))?;
        let mut sink = S3Sink::with_store(Arc::new(store), locator)?;
        sink.redact = [
            Some(&settings.secret_access_key),
            settings.session_token.as_ref(),
        ]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_empty())
        .cloned()
        .collect();
        Ok(sink)
    }

    /// The same sink over any object store; unit tests use an in-memory one.
    pub fn with_store(store: Arc<dyn ObjectStore>, locator: &S3Locator) -> Result<S3Sink> {
        if !locator.prefix().is_empty() && !locator.prefix().split('/').all(survives_as_key_segment)
        {
            bail!(
                "S3 prefix '{}' of {locator} cannot be written as given: an empty segment, \
                 '.', '..' or a character the S3 client percent-encodes would change the keys \
                 it is stored under",
                locator.prefix()
            );
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("xdu-s3")
            .enable_all()
            .build()
            .context("cannot start the S3 client's runtime")?;
        Ok(S3Sink {
            store,
            runtime,
            locator: locator.clone(),
            redact: Vec::new(),
        })
    }

    /// The key of `name` directly under the index prefix, or of `partition/name`.
    fn key(&self, partition: Option<&str>, name: &str) -> ObjectPath {
        let prefix = self.locator.prefix();
        let segments = prefix
            .split('/')
            .filter(|_| !prefix.is_empty())
            .chain(partition)
            .chain([name]);
        ObjectPath::from_iter(segments)
    }

    fn chunk_key(&self, partition: &str, id: usize) -> ObjectPath {
        self.key(Some(partition), &chunk_final_name(id))
    }

    fn marker_key(&self) -> ObjectPath {
        self.key(None, COMPLETION_MARKER)
    }

    fn prefix_path(&self) -> Option<ObjectPath> {
        let prefix = self.locator.prefix();
        (!prefix.is_empty()).then(|| ObjectPath::from_iter(prefix.split('/')))
    }

    /// The ids of `partition`'s chunks, ascending. Only keys directly under the partition that
    /// are exactly a chunk name count: a sibling partition sharing the name as a prefix, a
    /// deeper key and any foreign object are left alone.
    fn chunk_ids(&self, partition: &str) -> Result<Vec<usize>> {
        let dir = ObjectPath::from_iter(
            self.prefix_path()
                .iter()
                .flat_map(|p| p.parts())
                .chain([PathPart::from(partition)]),
        );
        let dir_prefix = format!("{}/", dir.as_ref());
        let mut listing = self.store.list(Some(&dir));
        let mut ids = Vec::new();
        while let Some(meta) = self.runtime.block_on(listing.next()) {
            let meta =
                meta.map_err(|e| self.request_error(&format!("listing partition {partition}"), e))?;
            let Some(name) = meta.location.as_ref().strip_prefix(&dir_prefix) else {
                continue;
            };
            if let Some(id) = chunk_id(name) {
                ids.push(id);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    /// Name the cause of a failed request: the endpoint's certificate untrusted, the endpoint
    /// unreachable, the credentials refused, the bucket missing, or the store's own words.
    ///
    /// The store's words stop before the response body. An S3 error body can echo the request
    /// as signed, header values included, so a session token rides in a `SignatureDoesNotMatch`
    /// body and an `InvalidToken` body names the token outright. What is kept is the status
    /// and the body's error code, which `error_detail` reads out, and as a last guard the
    /// secret and token are struck from the text should a client's wording ever change.
    fn request_error(&self, what: &str, err: object_store::Error) -> anyhow::Error {
        let index = &self.locator;
        let status = match &err {
            object_store::Error::PermissionDenied { .. } => Some(403),
            object_store::Error::Unauthenticated { .. } => Some(401),
            object_store::Error::NotFound { .. } => Some(404),
            // A listing's failure reaches here as a generic error; its status is only in the
            // text of the error it wraps.
            _ => error_chain(&err).find_map(|e| status_in_text(&e.to_string())),
        };
        let detail = self.redacted(&error_detail(&err.to_string(), status));
        match status {
            Some(401 | 403) => anyhow!(
                "the S3 endpoint refused the credentials while {what} for {index}; check \
                 AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or the profile AWS_PROFILE names, \
                 and that they may write there ({detail})"
            ),
            Some(404) => anyhow!(
                "bucket '{}' does not exist at the S3 endpoint (HTTP 404 while {what} for {index})",
                index.bucket()
            ),
            // A refused certificate fails the connection, so it is named before the
            // unreachable endpoint it would otherwise read as.
            _ if let Some(cause) = untrusted_certificate(&err) => anyhow!(
                "the S3 endpoint's TLS certificate is not trusted while {what} for {index}; it \
                 must chain to a CA in the file AWS_CA_BUNDLE names, else the platform's CA \
                 bundle ({})",
                self.redacted(&cause)
            ),
            _ if is_unreachable(&err) => anyhow!(
                "cannot reach the S3 endpoint while {what} for {index}: no connection could be \
                 established; check AWS_ENDPOINT_URL_S3, AWS_ENDPOINT_URL or the profile's \
                 endpoint_url ({detail})"
            ),
            _ => anyhow!("S3 request failed while {what} for {index}: {detail}"),
        }
    }

    fn redacted(&self, text: &str) -> String {
        self.redact.iter().fold(text.to_string(), |text, value| {
            text.replace(value.as_str(), "<redacted>")
        })
    }
}

/// Where the S3 client's error text starts quoting the response body.
const BODY_LEAD: &str = "Server returned ";

/// A failed request's text with the response body left out: the client's words up to where
/// it would quote the body, then the HTTP status and the body's S3 error code, if any.
fn error_detail(text: &str, status: Option<u16>) -> String {
    let (head, body) = match text.find(BODY_LEAD) {
        Some(at) => (&text[..at], &text[at..]),
        None => (text, ""),
    };
    let mut detail = head
        .trim_end()
        .trim_end_matches(['-', ':'])
        .trim_end()
        .to_string();
    let mut tail: Vec<String> = Vec::new();
    if let Some(status) = status {
        tail.push(format!("HTTP {status}"));
    }
    if let Some(code) = s3_error_code(body) {
        tail.push(code.to_string());
    }
    if !tail.is_empty() {
        if !detail.is_empty() {
            detail.push_str(" - ");
        }
        detail.push_str(&tail.join(" "));
    }
    detail
}

/// The `<Code>` of an S3 error body, when it is a plain identifier such as `AccessDenied`.
fn s3_error_code(body: &str) -> Option<&str> {
    let start = body.find("<Code>")? + "<Code>".len();
    let len = body[start..].find("</Code>")?;
    let code = &body[start..start + len];
    (!code.is_empty() && code.len() <= 64 && code.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then_some(code)
}

/// `err` and every error beneath it.
fn error_chain<'a>(
    err: &'a (dyn std::error::Error + 'static),
) -> impl Iterator<Item = &'a (dyn std::error::Error + 'static)> {
    std::iter::successors(Some(err), |e| e.source())
}

/// The HTTP status a store error's text reports, as the S3 client words it.
fn status_in_text(text: &str) -> Option<u16> {
    const LEAD: &str = "status code: ";
    let at = text.find(LEAD)? + LEAD.len();
    let digits = text.get(at..at + 3)?;
    digits
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| digits.parse().ok())
        .flatten()
}

/// The words of the TLS stack's refusal of the server's certificate, if the error chain holds
/// one: the deepest error that says so.
fn untrusted_certificate(err: &object_store::Error) -> Option<String> {
    const REFUSAL: &str = "invalid peer certificate";
    error_chain(err)
        .map(|link| link.to_string())
        .filter(|text| text.contains(REFUSAL))
        .last()
        .map(|text| text[text.find(REFUSAL).unwrap_or(0)..].to_string())
}

/// Whether the error chain ends in a failure to connect, or a timeout.
fn is_unreachable(err: &object_store::Error) -> bool {
    error_chain(err).any(|e| {
        e.downcast_ref::<HttpError>()
            .is_some_and(|h| matches!(h.kind(), HttpErrorKind::Connect | HttpErrorKind::Timeout))
    })
}

/// The id a chunk key's final segment names, if it is exactly a chunk name: `1.parquet` or
/// `000001.parquet.partial` is not chunk 1.
fn chunk_id(name: &str) -> Option<usize> {
    let stem = name.strip_suffix(".parquet")?;
    if stem.is_empty() || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id: usize = stem.parse().ok()?;
    (chunk_final_name(id) == name).then_some(id)
}

impl IndexSink for S3Sink {
    /// Reach the bucket with the credentials: one LIST of the index prefix's first page. An
    /// empty or absent prefix is a new index and passes; an unreachable endpoint, rejected
    /// credentials or a missing bucket fail, before anything is written.
    fn preflight(&self) -> Result<()> {
        let prefix = self.prefix_path();
        let mut listing = self.store.list(prefix.as_ref());
        match self.runtime.block_on(listing.next()) {
            None | Some(Ok(_)) => Ok(()),
            Some(Err(err)) => Err(self.request_error("listing the index prefix", err)),
        }
    }

    fn check_top_level(&self, entries: &[TopEntry], top_dir: &Path) -> Result<()> {
        check_s3_partition_names(entries, top_dir)
    }

    /// The first write of a run, so credentials that may read but not write fail here, with
    /// the marker still in place. A missing marker is not an error.
    fn clear_marker(&self) -> Result<()> {
        match self.runtime.block_on(self.store.delete(&self.marker_key())) {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(err) => Err(self.request_error("removing the completion marker", err)),
        }
    }

    /// One PUT at the chunk's final key. A single PUT is all or nothing, so no reader can see
    /// part of a chunk; until the marker is written, no reader accepts the index at all.
    fn put_chunk(&self, partition: &str, id: usize, bytes: Vec<u8>) -> Result<()> {
        let key = self.chunk_key(partition, id);
        self.runtime
            .block_on(self.store.put(&key, bytes::Bytes::from(bytes).into()))
            .map(|_| ())
            .map_err(|e| self.request_error(&format!("writing chunk {key}"), e))
    }

    /// Every chunk is already at its final key; delete each listed chunk numbered `count` or
    /// higher, past any gap, so no stale chunk survives in the partition.
    fn finalize_partition(&self, partition: &str, count: usize) -> Result<usize> {
        let mut pruned = 0;
        for id in self.chunk_ids(partition)? {
            if id < count {
                continue;
            }
            let key = self.chunk_key(partition, id);
            self.runtime
                .block_on(self.store.delete(&key))
                .map_err(|e| self.request_error(&format!("removing stale chunk {key}"), e))?;
            pruned += 1;
        }
        Ok(pruned)
    }

    fn write_marker(&self, contents: &str) -> Result<()> {
        let body = bytes::Bytes::copy_from_slice(contents.as_bytes());
        self.runtime
            .block_on(self.store.put(&self.marker_key(), body.into()))
            .map(|_| ())
            .map_err(|e| self.request_error("writing the completion marker", e))
    }

    fn marker_location(&self) -> String {
        self.locator.object_url(COMPLETION_MARKER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::ffi::OsStringExt;

    use tempfile::TempDir;

    const SECRET: &str = "s3cr3t-Vb8pQ2-never-printed";
    const TOKEN: &str = "t0k3n-Lm4wX9-never-printed";

    fn locator(raw: &str) -> Result<IndexLocator> {
        IndexLocator::parse(OsStr::new(raw))
    }

    fn s3(raw: &str) -> S3Locator {
        match locator(raw).unwrap() {
            IndexLocator::S3(l) => l,
            other => panic!("{raw} parsed as {other:?}"),
        }
    }

    #[test]
    fn s3_locator_splits_bucket_and_prefix() {
        let l = s3("s3://bucket/some/prefix");
        assert_eq!(l.bucket(), "bucket");
        assert_eq!(l.prefix(), "some/prefix");
        assert_eq!(l.to_string(), "s3://bucket/some/prefix");
    }

    #[test]
    fn trailing_slashes_name_the_same_index() {
        assert_eq!(s3("s3://bucket/p/"), s3("s3://bucket/p"));
        assert_eq!(s3("s3://bucket/p//").prefix(), "p");
        let root = s3("s3://bucket/");
        assert_eq!(root.prefix(), "");
        assert_eq!(root, s3("s3://bucket"));
        assert_eq!(root.to_string(), "s3://bucket");
    }

    #[test]
    fn empty_bucket_is_refused() {
        for raw in ["s3://", "s3:///prefix"] {
            let err = locator(raw).unwrap_err().to_string();
            assert!(err.contains("names no bucket"), "{raw}: {err}");
        }
    }

    #[test]
    fn userinfo_is_refused_without_echoing_it() {
        let err = locator(&format!("s3://key:{SECRET}@bucket/p"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("must not carry credentials"), "{err}");
        assert!(!err.contains(SECRET), "the error echoed the secret: {err}");
    }

    #[test]
    fn glob_metacharacters_are_refused() {
        for raw in [
            "s3://bucket/p*",
            "s3://bucket/a?b",
            "s3://bucket/[ab]",
            "s3://bucket/{a,b}",
            "s3://bucket/p]",
        ] {
            let err = locator(raw).unwrap_err().to_string();
            assert!(err.contains("glob metacharacter"), "{raw}: {err}");
        }
        // In the bucket, the bucket name rules refuse them first.
        for c in GLOB_METACHARACTERS {
            let raw = format!("s3://buck{c}et/p");
            let err = locator(&raw).unwrap_err().to_string();
            assert!(err.contains("bucket"), "{raw}: {err}");
        }
    }

    #[test]
    fn bucket_names_follow_s3s_rules() {
        let longest = "a".repeat(63);
        for bucket in [
            "xdu-test",
            "abc",
            "a.b-c",
            "127.0.0",
            "0bucket9",
            longest.as_str(),
        ] {
            assert_eq!(s3(&format!("s3://{bucket}/p")).bucket(), bucket);
        }
        let too_long = "a".repeat(64);
        for (bucket, expected) in [
            ("127.0.0.1:9000#", "contains ':'"),
            ("bucket#", "contains '#'"),
            ("bucket?x", "contains '?'"),
            ("buck\\et", "contains '\\\\'"),
            ("buck%2fet", "contains '%'"),
            ("buck et", "contains ' '"),
            ("bucket\t", "contains '\\t'"),
            ("bucket\n", "contains '\\n'"),
            ("Bucket", "contains 'B'"),
            ("buck_et", "contains '_'"),
            ("bück", "contains 'ü'"),
            ("ab", "is 2 character(s) long"),
            (too_long.as_str(), "is 64 character(s) long"),
            ("-bucket", "begins or ends"),
            ("bucket.", "begins or ends"),
            ("buck..et", "two adjacent '.'"),
        ] {
            let err = locator(&format!("s3://{bucket}/p"))
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected), "{bucket:?}: {err}");
        }
    }

    /// Every character the parser lets into a bucket is one S3 allows, so none can carry the
    /// writer's or a reader's request to a host the resolver did not name.
    #[test]
    fn no_character_outside_the_bucket_rules_reaches_the_host() {
        let settings = S3Settings {
            access_key_id: "id".to_string(),
            secret_access_key: SECRET.to_string(),
            session_token: None,
            region: DEFAULT_REGION.to_string(),
            endpoint: None,
            path_style: false,
            trust: None,
        };
        let suffix = format!(".{}", settings.endpoint_authority());
        for c in (0u32..0x300).filter_map(char::from_u32) {
            let bucket = format!("ab{c}cd");
            let raw = format!("s3://{bucket}/p");
            let allowed = c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-';
            match locator(&raw) {
                Ok(IndexLocator::S3(l)) => {
                    assert!(allowed, "{c:?} was let into a bucket");
                    let url = settings.client_endpoint_url(l.bucket());
                    let host = url.strip_prefix("https://").unwrap();
                    assert!(host.ends_with(&suffix), "{c:?}: {url}");
                    assert!(
                        host[..host.len() - suffix.len()]
                            .bytes()
                            .all(|b| b.is_ascii_lowercase()
                                || b.is_ascii_digit()
                                || b == b'.'
                                || b == b'-'),
                        "{c:?}: {url}"
                    );
                }
                Ok(other) => panic!("{raw:?} parsed as {other:?}"),
                // A `/` ends the bucket, so `ab/cd` names the bucket `ab`, which is too short.
                Err(_) => assert!(!allowed, "{c:?} was refused"),
            }
        }
    }

    /// Credentials typed into a locator never reach an error, whatever else is wrong with it:
    /// not valid UTF-8, a secret holding `/` (as AWS secret keys can) so it spills past the
    /// bucket, or a refusal that comes after the credentials check would have run.
    #[test]
    fn no_locator_refusal_prints_the_credentials() {
        const KEY: &str = "AKIDEXAMPLE";
        let cases: Vec<OsString> = vec![
            OsString::from_vec(format!("s3://{KEY}:{SECRET}@bucket/p\u{0}").into_bytes()),
            OsString::from_vec(
                [format!("s3://{KEY}:{SECRET}@bucket/p").as_bytes(), b"\xff"].concat(),
            ),
            OsString::from_vec(
                [format!("s3://{KEY}:{SECRET}").as_bytes(), b"\xff@bucket/p"].concat(),
            ),
            OsString::from(format!("s3://{KEY}:{SECRET}@bucket/p")),
            OsString::from(format!("s3://{KEY}:sec/{SECRET}@bucket/p")),
            OsString::from(format!("s3://{KEY}:sec/{SECRET}@bucket/p*")),
            OsString::from(format!("s3://{KEY}:sec/{SECRET}@bucket/p\u{0}")),
            OsString::from_vec(
                [
                    format!("s3://{KEY}:sec/{SECRET}@bucket/p").as_bytes(),
                    b"\xff",
                ]
                .concat(),
            ),
            OsString::from(format!("s3://:sec/{SECRET}@bucket/p")),
            OsString::from(format!("s3://{KEY}:@bucket/{SECRET}")),
        ];
        for raw in &cases {
            let err = format!("{:#}", IndexLocator::parse(raw).unwrap_err());
            for part in [KEY, SECRET, "sec/", "@bucket"] {
                assert!(
                    !err.contains(part),
                    "{raw:?}: the error printed {part:?}: {err}"
                );
            }
        }
    }

    #[test]
    fn a_quote_in_the_prefix_is_a_valid_locator() {
        // Bound parameters carry it to SQL; refusing it would be the wrong fix.
        assert_eq!(s3("s3://bucket/it's").prefix(), "it's");
    }

    #[test]
    fn uppercase_scheme_stays_local() {
        assert_eq!(
            locator("S3://bucket/p").unwrap(),
            IndexLocator::Local(PathBuf::from("S3://bucket/p"))
        );
    }

    #[test]
    fn plain_paths_stay_local() {
        for raw in [
            "/index/scratch",
            "relative/index",
            "s3:/bucket",
            "./s3://b/p",
        ] {
            assert_eq!(
                locator(raw).unwrap(),
                IndexLocator::Local(PathBuf::from(raw))
            );
        }
    }

    #[test]
    fn non_utf8_path_stays_local_byte_for_byte() {
        let raw = OsString::from_vec(b"/index/\xff\xfe".to_vec());
        let parsed = IndexLocator::parse(&raw).unwrap();
        assert_eq!(parsed, IndexLocator::Local(PathBuf::from(raw.clone())));
        assert!(!is_s3_locator(&raw));
    }

    #[test]
    fn non_utf8_s3_locator_is_refused() {
        let raw = OsString::from_vec(b"s3://bucket/\xff".to_vec());
        assert!(is_s3_locator(&raw));
        let err = IndexLocator::parse(&raw).unwrap_err().to_string();
        assert!(err.contains("not valid UTF-8"), "{err}");
    }

    /// Variables from a map; files from disk, except the ones the fixture stands in for.
    ///
    /// The platform CA bundles are always stood in for, so no test depends on the host's: each
    /// is absent but the last, which holds `test_ca()`'s certificate.
    struct FixtureEnv {
        vars: HashMap<String, OsString>,
        files: HashMap<PathBuf, std::result::Result<String, io::ErrorKind>>,
    }

    impl FixtureEnv {
        fn new(vars: &[(&str, &str)]) -> Self {
            let mut files: HashMap<_, _> = PLATFORM_CA_BUNDLES
                .iter()
                .map(|bundle| (PathBuf::from(bundle), Err(io::ErrorKind::NotFound)))
                .collect();
            files.insert(
                PathBuf::from(PLATFORM_CA_BUNDLES[PLATFORM_CA_BUNDLES.len() - 1]),
                Ok(test_ca().0.clone()),
            );
            FixtureEnv {
                vars: vars
                    .iter()
                    .map(|(k, v)| (k.to_string(), OsString::from(v)))
                    .collect(),
                files,
            }
        }

        /// `file` reads as `contents`, or fails with that kind.
        fn with_file(
            mut self,
            file: &str,
            contents: std::result::Result<&str, io::ErrorKind>,
        ) -> Self {
            self.files
                .insert(PathBuf::from(file), contents.map(str::to_string));
            self
        }
    }

    impl EnvSource for FixtureEnv {
        fn var(&self, name: &str) -> Option<OsString> {
            self.vars.get(name).cloned()
        }

        fn read_file(&self, file: &Path) -> io::Result<String> {
            match self.files.get(file) {
                Some(Ok(text)) => Ok(text.clone()),
                Some(Err(kind)) => Err(io::Error::from(*kind)),
                None => fs::read_to_string(file),
            }
        }
    }

    /// A temporary HOME-less world with the two shared files at fixed paths.
    struct Files {
        dir: TempDir,
    }

    impl Files {
        fn new(credentials: &str, config: &str) -> Self {
            let dir = TempDir::new().unwrap();
            fs::write(dir.path().join("credentials"), credentials).unwrap();
            fs::write(dir.path().join("config"), config).unwrap();
            Files { dir }
        }

        fn credentials(&self) -> String {
            self.dir.path().join("credentials").display().to_string()
        }

        fn config(&self) -> String {
            self.dir.path().join("config").display().to_string()
        }

        fn env(&self, extra: &[(&str, &str)]) -> FixtureEnv {
            let credentials = self.credentials();
            let config = self.config();
            let mut vars = vec![
                ("AWS_SHARED_CREDENTIALS_FILE", credentials.as_str()),
                ("AWS_CONFIG_FILE", config.as_str()),
            ];
            vars.extend_from_slice(extra);
            FixtureEnv::new(&vars)
        }
    }

    fn resolve(env: &FixtureEnv) -> S3Settings {
        S3Settings::resolve(env).unwrap()
    }

    fn resolve_err(env: &FixtureEnv) -> String {
        format!("{:#}", S3Settings::resolve(env).unwrap_err())
    }

    const CREDENTIALS: &str = "\
[default]
aws_access_key_id = file-default-id
aws_secret_access_key = file-default-secret

[work]
aws_access_key_id = file-work-id
aws_secret_access_key = file-work-secret
aws_session_token = file-work-token
";

    const CONFIG: &str = "\
# A comment line.
[default]
region = eu-west-1
aws_access_key_id = config-default-id
aws_secret_access_key = config-default-secret

[profile work]
region = ap-south-1
services = local-store

[profile onlyconfig]
aws_access_key_id = config-only-id
aws_secret_access_key = config-only-secret
endpoint_url = http://profile.example:9000

[services local-store]
s3 =
  endpoint_url = http://services.example:7480
";

    #[test]
    fn environment_credentials_beat_the_profile() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let s = resolve(&files.env(&[
            ("AWS_ACCESS_KEY_ID", "env-id"),
            ("AWS_SECRET_ACCESS_KEY", SECRET),
            ("AWS_SESSION_TOKEN", TOKEN),
        ]));
        assert_eq!(s.access_key_id, "env-id");
        assert_eq!(s.secret_access_key, SECRET);
        assert_eq!(s.session_token.as_deref(), Some(TOKEN));
    }

    #[test]
    fn credentials_file_beats_config_file() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let s = resolve(&files.env(&[]));
        assert_eq!(s.access_key_id, "file-default-id");
        assert_eq!(s.secret_access_key, "file-default-secret");
        assert_eq!(s.session_token, None);
    }

    #[test]
    fn config_file_credentials_serve_when_the_credentials_file_has_none() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let s = resolve(&files.env(&[("AWS_PROFILE", "onlyconfig")]));
        assert_eq!(s.access_key_id, "config-only-id");
        assert_eq!(s.secret_access_key, "config-only-secret");
    }

    #[test]
    fn aws_profile_selects_both_section_forms() {
        // `[work]` in the credentials file, `[profile work]` in the config file.
        let files = Files::new(CREDENTIALS, CONFIG);
        let s = resolve(&files.env(&[("AWS_PROFILE", "work")]));
        assert_eq!(s.access_key_id, "file-work-id");
        assert_eq!(s.session_token.as_deref(), Some("file-work-token"));
        assert_eq!(s.region, "ap-south-1");
    }

    #[test]
    fn a_bare_section_in_the_config_file_is_not_a_profile() {
        let files = Files::new("", "[work]\nregion = ap-south-1\n");
        let err = resolve_err(&files.env(&[("AWS_PROFILE", "work")]));
        assert!(err.contains("neither"), "{err}");
    }

    /// Two profiles in the credentials file, the second header written `header_tail`.
    fn two_profile_credentials(header_tail: &str) -> String {
        format!(
            "[default]\naws_access_key_id = default-id\naws_secret_access_key = default-key\n\n\
             [other]{header_tail}\naws_access_key_id = other-id\naws_secret_access_key = other-key\n"
        )
    }

    #[test]
    fn a_comment_after_a_section_header_keeps_the_profiles_apart() {
        for tail in [
            " # second account",
            " ; second account",
            "# tight",
            "\t; tab",
        ] {
            let files = Files::new(&two_profile_credentials(tail), "");
            let default = resolve(&files.env(&[]));
            assert_eq!(default.access_key_id, "default-id", "{tail:?}");
            assert_eq!(default.secret_access_key, "default-key", "{tail:?}");
            let other = resolve(&files.env(&[("AWS_PROFILE", "other")]));
            assert_eq!(other.access_key_id, "other-id", "{tail:?}");
            assert_eq!(other.secret_access_key, "other-key", "{tail:?}");
        }
        // The config file's `[profile name]` form, with a comment on the header.
        let files = Files::new(
            "",
            "[default]\nregion = eu-west-1\n[profile other] # staging\nregion = ap-south-1\n\
             aws_access_key_id = cfg-id\naws_secret_access_key = cfg-key\n",
        );
        let other = resolve(&files.env(&[("AWS_PROFILE", "other")]));
        assert_eq!(other.region, "ap-south-1");
        assert_eq!(other.access_key_id, "cfg-id");
    }

    #[test]
    fn a_malformed_section_header_takes_no_keys_from_or_into_another_profile() {
        // Text after the closing bracket that is not a comment, or no closing bracket: the
        // keys under it belong to no profile, never to the one above.
        for tail in [" trailing", "x"] {
            let files = Files::new(&two_profile_credentials(tail), "");
            let default = resolve(&files.env(&[]));
            assert_eq!(default.access_key_id, "default-id", "{tail:?}");
            assert_eq!(default.secret_access_key, "default-key", "{tail:?}");
        }
        let unclosed = "[default]\naws_access_key_id = default-id\naws_secret_access_key = default-key\n\
                        [other\naws_access_key_id = other-id\naws_secret_access_key = other-key\n";
        let files = Files::new(unclosed, "");
        assert_eq!(resolve(&files.env(&[])).access_key_id, "default-id");
        let err = resolve_err(&files.env(&[("AWS_PROFILE", "other")]));
        assert!(err.contains("neither"), "{err}");
    }

    #[test]
    fn section_headers_follow_the_shared_file_rules() {
        // Whitespace inside the brackets is not part of the name.
        let files = Files::new(
            "[ spaced ]\naws_access_key_id = spaced-id\naws_secret_access_key = spaced-key\n",
            "[profile   wide]\nregion = ap-south-1\n\
             aws_access_key_id = wide-id\naws_secret_access_key = wide-key\n",
        );
        assert_eq!(
            resolve(&files.env(&[("AWS_PROFILE", "spaced")])).access_key_id,
            "spaced-id"
        );
        assert_eq!(
            resolve(&files.env(&[("AWS_PROFILE", "wide")])).region,
            "ap-south-1"
        );
        // Profile names are case-sensitive, and so is the `profile` prefix; keys are not.
        let files = Files::new(
            "[Work]\nAWS_ACCESS_KEY_ID = upper-id\nAws_Secret_Access_Key = upper-key\n",
            "[Profile lower]\nregion = ap-south-1\n",
        );
        assert_eq!(
            resolve(&files.env(&[("AWS_PROFILE", "Work")])).access_key_id,
            "upper-id"
        );
        let err = resolve_err(&files.env(&[("AWS_PROFILE", "work")]));
        assert!(err.contains("neither"), "{err}");
        let err = resolve_err(&files.env(&[("AWS_PROFILE", "lower")]));
        assert!(err.contains("neither"), "{err}");
        // `[profile name]` belongs to the config file only: in the credentials file it names no
        // profile, so its keys are not the profile's.
        let files = Files::new(
            "[profile cred]\naws_access_key_id = wrong-id\naws_secret_access_key = wrong-key\n",
            "",
        );
        let err = resolve_err(&files.env(&[("AWS_PROFILE", "cred")]));
        assert!(err.contains("neither"), "{err}");
        // In the config file `[profile default]` is the default profile, ahead of `[default]`.
        let files = Files::new(
            "",
            "[default]\nregion = eu-west-1\n[profile default]\nregion = ap-south-1\n\
             aws_access_key_id = pd-id\naws_secret_access_key = pd-key\n",
        );
        assert_eq!(resolve(&files.env(&[])).region, "ap-south-1");
    }

    #[test]
    fn region_precedence() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let both = [
            ("AWS_REGION", "from-region"),
            ("AWS_DEFAULT_REGION", "from-default-region"),
        ];
        assert_eq!(resolve(&files.env(&both)).region, "from-region");
        assert_eq!(
            resolve(&files.env(&both[1..])).region,
            "from-default-region"
        );
        assert_eq!(resolve(&files.env(&[])).region, "eu-west-1");
        let bare = Files::new(CREDENTIALS, "");
        assert_eq!(resolve(&bare.env(&[])).region, DEFAULT_REGION);
    }

    #[test]
    fn endpoint_precedence() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let all = [
            ("AWS_PROFILE", "work"),
            ("AWS_ENDPOINT_URL_S3", "http://s3-specific.example:1"),
            ("AWS_ENDPOINT_URL", "http://general.example:2"),
        ];
        let url = |vars: &[(&str, &str)]| {
            resolve(&files.env(vars))
                .endpoint
                .map(|e| e.url())
                .unwrap_or_default()
        };
        assert_eq!(url(&all), "http://s3-specific.example:1");
        assert_eq!(
            url(&[all[0], all[2]]),
            "http://general.example:2",
            "AWS_ENDPOINT_URL serves when AWS_ENDPOINT_URL_S3 is unset"
        );
        assert_eq!(
            url(&all[..1]),
            "http://services.example:7480",
            "the services section serves when no variable is set"
        );
        assert_eq!(
            url(&[("AWS_PROFILE", "onlyconfig")]),
            "http://profile.example:9000",
            "the profile endpoint_url serves last"
        );
    }

    #[test]
    fn path_style_iff_an_endpoint_is_set() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let aws = resolve(&files.env(&[]));
        assert!(aws.endpoint.is_none());
        assert!(!aws.path_style);
        let local = resolve(&files.env(&[("AWS_ENDPOINT_URL", "http://127.0.0.1:9000")]));
        assert!(local.path_style);
        let endpoint = local.endpoint.unwrap();
        assert_eq!(endpoint.authority(), "127.0.0.1:9000");
        assert!(!endpoint.use_ssl());
        let tls = resolve(&files.env(&[("AWS_ENDPOINT_URL", "https://store.example/")]));
        assert!(tls.endpoint.unwrap().use_ssl());
    }

    #[test]
    fn half_set_environment_credentials_are_named_errors() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let err = resolve_err(&files.env(&[("AWS_ACCESS_KEY_ID", "env-id")]));
        assert!(err.contains("AWS_SECRET_ACCESS_KEY is not"), "{err}");
        let err = resolve_err(&files.env(&[("AWS_SECRET_ACCESS_KEY", SECRET)]));
        assert!(err.contains("AWS_ACCESS_KEY_ID is not"), "{err}");
        assert!(!err.contains(SECRET), "the error echoed the secret: {err}");
        let err = resolve_err(&files.env(&[("AWS_SESSION_TOKEN", TOKEN)]));
        assert!(err.contains("AWS_SESSION_TOKEN is set without"), "{err}");
        assert!(!err.contains(TOKEN), "the error echoed the token: {err}");
    }

    #[test]
    fn half_set_profile_credentials_are_named_errors() {
        let files = Files::new(
            &format!("[default]\naws_secret_access_key = {SECRET}\n"),
            "",
        );
        let err = resolve_err(&files.env(&[]));
        assert!(err.contains("holds only one of"), "{err}");
        assert!(err.contains(&files.credentials()), "{err}");
        assert!(!err.contains(SECRET), "the error echoed the secret: {err}");
    }

    #[test]
    fn no_credentials_is_a_named_error() {
        let files = Files::new("", "[default]\nregion = eu-west-1\n");
        let err = resolve_err(&files.env(&[]));
        assert!(err.contains("no S3 credentials"), "{err}");
        assert!(err.contains(&files.credentials()), "{err}");
        assert!(err.contains(&files.config()), "{err}");
        let nothing = FixtureEnv::new(&[]);
        let err = format!("{:#}", S3Settings::resolve(&nothing).unwrap_err());
        assert!(err.contains("HOME is unset"), "{err}");
    }

    #[test]
    fn a_missing_named_profile_is_an_error() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let err = resolve_err(&files.env(&[
            ("AWS_PROFILE", "nope"),
            ("AWS_ACCESS_KEY_ID", "env-id"),
            ("AWS_SECRET_ACCESS_KEY", SECRET),
        ]));
        assert!(err.contains("profile 'nope'"), "{err}");
        assert!(!err.contains(SECRET), "the error echoed the secret: {err}");
    }

    #[test]
    fn absent_shared_files_are_empty() {
        let dir = TempDir::new().unwrap();
        let home = dir.path().display().to_string();
        let s = S3Settings::resolve(&FixtureEnv::new(&[
            ("HOME", home.as_str()),
            ("AWS_ACCESS_KEY_ID", "env-id"),
            ("AWS_SECRET_ACCESS_KEY", SECRET),
        ]))
        .unwrap();
        assert_eq!(s.region, DEFAULT_REGION);
    }

    #[test]
    fn home_locates_the_shared_files_when_no_variable_does() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".aws")).unwrap();
        fs::write(dir.path().join(".aws/credentials"), CREDENTIALS).unwrap();
        fs::write(dir.path().join(".aws/config"), CONFIG).unwrap();
        let home = dir.path().display().to_string();
        let s = S3Settings::resolve(&FixtureEnv::new(&[("HOME", home.as_str())])).unwrap();
        assert_eq!(s.access_key_id, "file-default-id");
        assert_eq!(s.region, "eu-west-1");
    }

    #[test]
    fn bad_endpoints_are_named_errors_without_their_value() {
        let files = Files::new(CREDENTIALS, CONFIG);
        for (raw, reason) in [
            ("http://host:9000/bucket", "carries a path"),
            ("http://host/?x=1", "carries a path"),
            ("ftp://host", "not an http"),
            ("host:9000", "not an http"),
            ("http://user:pw-Kd93hQ@host", "carries user information"),
            ("http://", "names no host"),
            ("http://host:port", "port that is not a number"),
            ("http://good\\evil.example:9000", "a host holding"),
            ("http://good evil.example", "a host holding"),
            ("http://good\tevil.example", "a host holding"),
            ("http://g%6Fod.example", "a host holding"),
            ("http://[::1%25en0]:9000", "a host holding"),
        ] {
            let err = resolve_err(&files.env(&[("AWS_ENDPOINT_URL_S3", raw)]));
            assert!(err.contains("AWS_ENDPOINT_URL_S3"), "{raw}: {err}");
            assert!(err.contains(reason), "{raw}: {err}");
            assert!(
                !err.contains(raw),
                "{raw}: the error echoed the value: {err}"
            );
        }
        let bad_profile = Files::new(
            CREDENTIALS,
            "[default]\nendpoint_url = http://host:1/path\n",
        );
        let err = resolve_err(&bad_profile.env(&[]));
        assert!(err.contains("endpoint_url of profile 'default'"), "{err}");
        assert!(err.contains(&bad_profile.config()), "{err}");
    }

    #[test]
    fn an_ipv6_endpoint_keeps_its_brackets() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let s = resolve(&files.env(&[("AWS_ENDPOINT_URL", "http://[::1]:9000")]));
        assert_eq!(s.endpoint.unwrap().authority(), "[::1]:9000");
    }

    #[test]
    fn debug_redacts_the_secret_and_token() {
        let files = Files::new(CREDENTIALS, CONFIG);
        let s = resolve(&files.env(&[
            ("AWS_ACCESS_KEY_ID", "env-id"),
            ("AWS_SECRET_ACCESS_KEY", SECRET),
            ("AWS_SESSION_TOKEN", TOKEN),
        ]));
        let printed = format!("{s:?} {s:#?}");
        assert!(!printed.contains(SECRET), "{printed}");
        assert!(!printed.contains(TOKEN), "{printed}");
        assert!(printed.contains("<redacted>"), "{printed}");
        assert!(printed.contains("env-id"), "{printed}");
    }

    fn settings(token: bool, endpoint: bool) -> S3Settings {
        let files = Files::new("", "");
        let mut vars = vec![
            ("AWS_ACCESS_KEY_ID", "key-id-AK19"),
            ("AWS_SECRET_ACCESS_KEY", SECRET),
        ];
        if token {
            vars.push(("AWS_SESSION_TOKEN", TOKEN));
        }
        if endpoint {
            vars.push(("AWS_ENDPOINT_URL", "http://127.0.0.1:9000"));
        }
        resolve(&files.env(&vars))
    }

    #[test]
    fn the_secret_statement_text_holds_no_credential() {
        for (token, endpoint) in [(false, false), (true, false), (false, true), (true, true)] {
            let s = settings(token, endpoint);
            let (sql, params) = s.duckdb_secret_statement();
            for value in [SECRET, TOKEN, "key-id-AK19", "127.0.0.1"] {
                assert!(!sql.contains(value), "{value} is in the SQL text: {sql}");
            }
            // Every placeholder has a value, and the secret is among the bound values: the
            // statement carries it, only never as text.
            assert_eq!(sql.matches('?').count(), params.len(), "{sql}");
            assert!(params.contains(&Value::Text(SECRET.to_string())));
            assert_eq!(
                params.contains(&Value::Text(TOKEN.to_string())),
                token,
                "{sql}"
            );
        }
    }

    /// The statement's clauses, each paired with the value bound to it.
    fn bound_clauses(s: &S3Settings) -> Vec<(String, Value)> {
        let (sql, params) = s.duckdb_secret_statement();
        let names: Vec<String> = sql
            .split([',', '(', ')'])
            .filter(|clause| clause.trim_end().ends_with('?'))
            .map(|clause| clause.trim().trim_end_matches('?').trim().to_string())
            .collect();
        assert_eq!(names.len(), params.len(), "{sql}");
        names.into_iter().zip(params).collect()
    }

    #[test]
    fn the_secret_statement_text_is_fixed_and_names_every_key_httpfs_would_fall_back_on() {
        // httpfs fills these settings from the environment when it loads and falls back on
        // any the secret leaves out, so each is bound whatever the resolver found.
        let expected = [
            "KEY_ID",
            "SECRET",
            "SESSION_TOKEN",
            "REGION",
            "ENDPOINT",
            "URL_STYLE",
            "USE_SSL",
            "REQUESTER_PAYS",
            "KMS_KEY_ID",
        ];
        let (first, _) = settings(false, false).duckdb_secret_statement();
        for (token, endpoint) in [(false, false), (true, false), (false, true), (true, true)] {
            let s = settings(token, endpoint);
            assert_eq!(s.duckdb_secret_statement().0, first);
            let names: Vec<String> = bound_clauses(&s).into_iter().map(|(n, _)| n).collect();
            assert_eq!(names, expected);
        }
    }

    #[test]
    fn the_secret_statement_addresses_by_endpoint() {
        let bound = |s: &S3Settings, name: &str| {
            bound_clauses(s)
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v)
                .unwrap()
        };
        let text = |v: &str| Value::Text(v.to_string());

        // No override: AWS's regional endpoint, by name, over TLS, virtual-hosted.
        let aws = settings(false, false);
        assert_eq!(bound(&aws, "ENDPOINT"), text("s3.us-east-1.amazonaws.com"));
        assert_eq!(bound(&aws, "URL_STYLE"), text("vhost"));
        assert_eq!(bound(&aws, "USE_SSL"), Value::Boolean(true));
        assert_eq!(bound(&aws, "SESSION_TOKEN"), text(""));
        assert_eq!(bound(&aws, "REQUESTER_PAYS"), Value::Boolean(false));
        assert_eq!(bound(&aws, "KMS_KEY_ID"), text(""));

        let local = settings(true, true);
        assert_eq!(bound(&local, "ENDPOINT"), text("127.0.0.1:9000"));
        assert_eq!(bound(&local, "URL_STYLE"), text("path"));
        assert_eq!(bound(&local, "USE_SSL"), Value::Boolean(false));
        assert_eq!(bound(&local, "SESSION_TOKEN"), text(TOKEN));
    }

    #[test]
    fn the_writer_and_the_readers_address_the_same_host() {
        let files = Files::new("", "");
        let creds = [
            ("AWS_ACCESS_KEY_ID", "key-id-AK19"),
            ("AWS_SECRET_ACCESS_KEY", SECRET),
        ];
        let with = |extra: &[(&str, &str)]| {
            let mut vars = creds.to_vec();
            vars.extend_from_slice(extra);
            resolve(&files.env(&vars))
        };

        let aws = with(&[("AWS_REGION", "eu-central-2")]);
        assert_eq!(aws.endpoint_authority(), "s3.eu-central-2.amazonaws.com");
        assert_eq!(
            aws.client_endpoint_url("bkt"),
            "https://bkt.s3.eu-central-2.amazonaws.com"
        );
        let (_, params) = aws.duckdb_secret_statement();
        assert!(params.contains(&Value::Text(aws.endpoint_authority())));

        let local = with(&[("AWS_ENDPOINT_URL_S3", "http://127.0.0.1:9000")]);
        assert_eq!(local.client_endpoint_url("bkt"), "http://127.0.0.1:9000");
        let (_, params) = local.duckdb_secret_statement();
        assert!(params.contains(&Value::Text("127.0.0.1:9000".to_string())));

        let tls = with(&[("AWS_ENDPOINT_URL", "https://store.example:8443")]);
        assert_eq!(tls.client_endpoint_url("bkt"), "https://store.example:8443");
        assert!(tls.use_ssl());
    }

    #[test]
    fn a_region_that_would_name_another_host_is_refused_for_aws() {
        let files = Files::new("", "");
        for region in [
            "evil.example/",
            "us-east-1.evil",
            "US-EAST-1",
            "-x",
            "a b",
            "x@y",
        ] {
            let err = resolve_err(&files.env(&[
                ("AWS_ACCESS_KEY_ID", "key-id-AK19"),
                ("AWS_SECRET_ACCESS_KEY", SECRET),
                ("AWS_REGION", region),
            ]));
            assert!(
                err.contains("AWS_REGION is not an AWS region name"),
                "{region}: {err}"
            );
            assert!(
                !err.contains(region),
                "{region}: the error echoed the value: {err}"
            );
            assert!(!err.contains(SECRET), "the error echoed the secret: {err}");
        }
        let bad_profile = Files::new("", "[default]\nregion = evil.example\n");
        let err = resolve_err(&bad_profile.env(&[
            ("AWS_ACCESS_KEY_ID", "key-id-AK19"),
            ("AWS_SECRET_ACCESS_KEY", SECRET),
        ]));
        assert!(err.contains("the region of profile 'default'"), "{err}");
        assert!(!err.contains("evil.example"), "{err}");
        // With an override the region only scopes the signature, as a store's own names do.
        let ok = resolve(&files.env(&[
            ("AWS_ACCESS_KEY_ID", "key-id-AK19"),
            ("AWS_SECRET_ACCESS_KEY", SECRET),
            ("AWS_REGION", "Garage.Local"),
            ("AWS_ENDPOINT_URL", "http://127.0.0.1:9000"),
        ]));
        assert_eq!(ok.region, "Garage.Local");
    }

    // ---- the writer ---------------------------------------------------------

    #[test]
    fn partition_names_that_survive_as_keys_and_globs_are_accepted() {
        for name in [
            "alice",
            "__root__",
            ".xdu-complete",
            ".hidden",
            "it's",
            "with space",
            "a+b=c,d;e",
            "...",
        ] {
            assert_eq!(s3_partition_name_problem(OsStr::new(name)), None, "{name}");
        }
    }

    #[test]
    fn partition_names_the_client_would_encode_are_refused() {
        for name in [
            "a~b",
            "a%b",
            "a#b",
            "a^b",
            "a|b",
            "a\\b",
            "a`b",
            "a\"b",
            "a<b",
            "a>b",
            "tab\there",
            ".",
            "..",
            "caf\u{e9}",
        ] {
            let problem = s3_partition_name_problem(OsStr::new(name));
            assert!(
                problem.is_some_and(|p| p.contains("percent-encodes")),
                "{name:?}: {problem:?}"
            );
        }
        for name in ["a*b", "a?b", "[ab]", "{a,b}", "a]"] {
            let problem = s3_partition_name_problem(OsStr::new(name));
            assert!(
                problem.is_some_and(|p| p.contains("glob metacharacter")),
                "{name:?}: {problem:?}"
            );
        }
        let raw = OsString::from_vec(b"bad\xffname".to_vec());
        assert_eq!(s3_partition_name_problem(&raw), Some("is not valid UTF-8"));
    }

    fn entry(name: &OsStr, is_dir: bool) -> TopEntry {
        TopEntry {
            path: Path::new("/data").join(name),
            name: name.to_string_lossy().to_string(),
            is_dir,
            is_file: !is_dir,
            is_symlink: false,
        }
    }

    #[test]
    fn the_name_check_names_the_directory_and_ignores_loose_files() {
        let ok = [
            entry(OsStr::new("alice"), true),
            entry(OsStr::new("a~b.txt"), false),
        ];
        check_s3_partition_names(&ok, Path::new("/data")).unwrap();
        let bad = [
            entry(OsStr::new("alice"), true),
            entry(OsStr::new("a~b"), true),
        ];
        let err = check_s3_partition_names(&bad, Path::new("/data"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("'a~b'") && err.contains("/data"), "{err}");
        assert!(err.contains("S3 index"), "{err}");
    }

    #[test]
    fn statuses_are_read_from_the_clients_error_text() {
        assert_eq!(
            status_in_text(
                "Error performing GET http://h/b?list-type=2 in 2ms - Server returned non-2xx \
                 status code: 403 Forbidden: <Error/>"
            ),
            Some(403)
        );
        assert_eq!(
            status_in_text("Server returned non-2xx status code: 404 Not Found: "),
            Some(404)
        );
        assert_eq!(status_in_text("error trying to connect: refused"), None);
        assert_eq!(status_in_text("status code: 4"), None);
    }

    #[test]
    fn error_detail_keeps_the_status_and_code_and_drops_the_body() {
        let text = "Generic S3 error: Error performing list request: Error performing GET \
                    http://h/b?list-type=2 in 2ms - Server returned non-2xx status code: 403 \
                    Forbidden: <Error><Code>SignatureDoesNotMatch</Code><CanonicalRequest>GET\n\
                    x-amz-security-token:t0k3n</CanonicalRequest></Error>";
        let detail = error_detail(text, Some(403));
        assert_eq!(
            detail,
            "Generic S3 error: Error performing list request: Error performing GET \
             http://h/b?list-type=2 in 2ms - HTTP 403 SignatureDoesNotMatch"
        );
        let response = "Error performing PUT http://h/b/k in 1ms - Server returned error \
                        response: <Error><Code>InvalidToken</Code><Token-0>t0k3n</Token-0></Error>";
        assert_eq!(
            error_detail(response, Some(400)),
            "Error performing PUT http://h/b/k in 1ms - HTTP 400 InvalidToken"
        );
        // No body: the text stands, with the status after it.
        assert_eq!(
            error_detail("error trying to connect: refused", None),
            "error trying to connect: refused"
        );
        // A code that is not a plain identifier is not repeated.
        assert_eq!(s3_error_code("<Code>t0k3n here</Code>"), None);
        assert_eq!(s3_error_code("<Code></Code>"), None);
        assert_eq!(
            s3_error_code("<Code>AccessDenied</Code>"),
            Some("AccessDenied")
        );
    }

    #[test]
    fn the_sinks_errors_strike_the_secret_and_token() {
        let (mut sink, _) = memory_sink("s3://bkt/idx");
        sink.redact = vec![SECRET.to_string(), TOKEN.to_string()];
        let err = object_store::Error::Generic {
            store: "S3",
            source: format!("echoed {SECRET} and {TOKEN}").into(),
        };
        let text = format!("{:#}", sink.request_error("testing", err));
        assert!(!text.contains(SECRET) && !text.contains(TOKEN), "{text}");
        assert!(text.contains("<redacted>"), "{text}");
    }

    #[test]
    fn chunk_ids_parse_only_exact_chunk_names() {
        assert_eq!(chunk_id("000042.parquet"), Some(42));
        assert_eq!(chunk_id("1000000.parquet"), Some(1_000_000));
        assert_eq!(chunk_id("1.parquet"), None);
        assert_eq!(chunk_id("000001.parquet.partial"), None);
        assert_eq!(chunk_id("sub/000001.parquet"), None);
        assert_eq!(chunk_id(".xdu-complete"), None);
        assert_eq!(chunk_id(".parquet"), None);
    }

    fn memory_sink(raw: &str) -> (S3Sink, Arc<object_store::memory::InMemory>) {
        let store = Arc::new(object_store::memory::InMemory::new());
        let sink = S3Sink::with_store(store.clone(), &s3(raw)).unwrap();
        (sink, store)
    }

    fn stored_keys(sink: &S3Sink, store: &object_store::memory::InMemory) -> Vec<String> {
        let mut keys: Vec<String> = sink
            .runtime
            .block_on(store.list(None).collect::<Vec<_>>())
            .into_iter()
            .map(|m| m.unwrap().location.to_string())
            .collect();
        keys.sort();
        keys
    }

    #[test]
    fn keys_follow_the_index_layout() {
        let (sink, _) = memory_sink("s3://bucket/some/prefix");
        assert_eq!(
            sink.chunk_key("alice", 3).as_ref(),
            "some/prefix/alice/000003.parquet"
        );
        assert_eq!(sink.marker_key().as_ref(), "some/prefix/.xdu-complete");
        assert_eq!(
            sink.marker_location(),
            "s3://bucket/some/prefix/.xdu-complete"
        );
        let (root, _) = memory_sink("s3://bucket");
        assert_eq!(
            root.chunk_key("__root__", 0).as_ref(),
            "__root__/000000.parquet"
        );
        assert_eq!(root.marker_key().as_ref(), ".xdu-complete");
    }

    #[test]
    fn a_prefix_the_client_would_rewrite_is_refused() {
        let store = Arc::new(object_store::memory::InMemory::new());
        for raw in [
            "s3://bucket/a~b",
            "s3://bucket/a//b",
            "s3://bucket//p",
            "s3://bucket/a/../b",
        ] {
            let err = S3Sink::with_store(store.clone(), &s3(raw))
                .err()
                .unwrap_or_else(|| panic!("{raw} was accepted"))
                .to_string();
            assert!(err.contains("cannot be written as given"), "{raw}: {err}");
        }
        S3Sink::with_store(store, &s3("s3://bucket/it's/idx")).unwrap();
    }

    #[test]
    fn the_sink_writes_prunes_past_gaps_and_clears() {
        let (sink, store) = memory_sink("s3://bucket/idx");
        sink.preflight().unwrap();
        // Clearing a marker that is not there is not an error.
        sink.clear_marker().unwrap();

        for id in [0, 1, 2, 5, 9] {
            sink.put_chunk("alice", id, b"old".to_vec()).unwrap();
        }
        sink.put_chunk("alice2", 7, b"sibling".to_vec()).unwrap();
        sink.put_chunk("alice", 1, b"new".to_vec()).unwrap();
        // Foreign objects under the partition are not chunks and stay.
        let foreign = ObjectPath::from("idx/alice/1.parquet");
        sink.runtime
            .block_on(store.put(&foreign, bytes::Bytes::from_static(b"x").into()))
            .unwrap();

        // A run that wrote two chunks removes every higher id, past the gaps at 3-4 and 6-8.
        assert_eq!(sink.finalize_partition("alice", 2).unwrap(), 3);
        sink.write_marker("format=2\n").unwrap();
        assert_eq!(
            stored_keys(&sink, &store),
            vec![
                "idx/.xdu-complete",
                "idx/alice/000000.parquet",
                "idx/alice/000001.parquet",
                "idx/alice/1.parquet",
                "idx/alice2/000007.parquet",
            ]
        );

        sink.clear_marker().unwrap();
        assert!(!stored_keys(&sink, &store).contains(&"idx/.xdu-complete".to_string()));
    }

    // ---------------------------------------------------------------------------------------
    // The trust source
    // ---------------------------------------------------------------------------------------

    /// A CA certificate made once per test process: its PEM and its DER.
    fn test_ca() -> &'static (String, CertificateDer<'static>) {
        static CA: std::sync::OnceLock<(String, CertificateDer<'static>)> =
            std::sync::OnceLock::new();
        CA.get_or_init(|| make_ca("xdu unit-test CA", false))
    }

    /// A self-signed CA certificate. With `refused_extension`, it also carries a critical
    /// extension no verifier knows: valid DER that webpki will not take as an anchor.
    fn make_ca(name: &str, refused_extension: bool) -> (String, CertificateDer<'static>) {
        use rcgen::{BasicConstraints, CertificateParams, CustomExtension, DnType, IsCa, KeyPair};
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.distinguished_name.push(DnType::CommonName, name);
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        if refused_extension {
            let mut extension =
                CustomExtension::from_oid_content(&[1, 3, 6, 1, 4, 1, 55555, 1], vec![0x05, 0x00]);
            extension.set_criticality(true);
            params.custom_extensions.push(extension);
        }
        let certificate = params.self_signed(&key).unwrap();
        (certificate.pem(), certificate.der().clone())
    }

    /// A CA certificate the test CA signed, and a leaf that intermediate signed for
    /// `IP:127.0.0.1`, made once per test process: each as its PEM and its DER.
    fn test_intermediate() -> &'static [(String, CertificateDer<'static>); 2] {
        use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
        static CHAIN: std::sync::OnceLock<[(String, CertificateDer<'static>); 2]> =
            std::sync::OnceLock::new();
        CHAIN.get_or_init(|| {
            let ca_key = KeyPair::generate().unwrap();
            let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
            ca_params
                .distinguished_name
                .push(DnType::CommonName, "xdu unit-test root CA");
            ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            let ca = Issuer::new(ca_params, ca_key);
            let key = KeyPair::generate().unwrap();
            let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
            params
                .distinguished_name
                .push(DnType::CommonName, "xdu unit-test intermediate CA");
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            let intermediate = params.signed_by(&key, &ca).unwrap();
            let intermediate_issuer = Issuer::new(params, key);
            let leaf_key = KeyPair::generate().unwrap();
            let leaf = CertificateParams::new(vec!["127.0.0.1".to_string()])
                .unwrap()
                .signed_by(&leaf_key, &intermediate_issuer)
                .unwrap();
            [
                (intermediate.pem(), intermediate.der().clone()),
                (leaf.pem(), leaf.der().clone()),
            ]
        })
    }

    /// A PEM block of `label` around `der`.
    fn pem_block(label: &str, der: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut body = String::new();
        for group in der.chunks(3) {
            let bits = group.iter().enumerate().fold(0u32, |bits, (i, byte)| {
                bits | u32::from(*byte) << (16 - 8 * i)
            });
            for i in 0..4 {
                if i <= group.len() {
                    body.push(char::from(ALPHABET[(bits >> (18 - 6 * i) & 0x3f) as usize]));
                } else {
                    body.push('=');
                }
            }
        }
        let lines: Vec<&str> = body
            .as_bytes()
            .chunks(64)
            .map(|line| std::str::from_utf8(line).unwrap())
            .collect();
        format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
            lines.join("\n")
        )
    }

    const KEY_PAIR: [(&str, &str); 2] = [
        ("AWS_ACCESS_KEY_ID", "key-id-AK19"),
        ("AWS_SECRET_ACCESS_KEY", SECRET),
    ];

    /// Resolve with the fixture key pair, `extra` and `files` in place.
    fn trust_env(extra: &[(&str, &str)]) -> FixtureEnv {
        let mut vars: Vec<(&str, &str)> = KEY_PAIR.to_vec();
        vars.push(("HOME", ""));
        vars.extend_from_slice(extra);
        FixtureEnv::new(&vars)
    }

    /// A temporary file holding `contents`, in a directory the caller keeps.
    fn bundle_file(dir: &TempDir, name: &str, contents: &str) -> String {
        let file = dir.path().join(name);
        fs::write(&file, contents).unwrap();
        file.to_str().unwrap().to_string()
    }

    /// The endpoints a trust source applies to: AWS's, and an `https://` override.
    const HTTPS_ENDPOINTS: [&[(&str, &str)]; 2] =
        [&[], &[("AWS_ENDPOINT_URL", "https://127.0.0.1:9000")]];

    #[test]
    fn the_trust_source_is_the_file_aws_ca_bundle_names() {
        let dir = TempDir::new().unwrap();
        let (pem, der) = test_ca();
        let file = bundle_file(&dir, "ca.pem", pem);
        for endpoint in HTTPS_ENDPOINTS {
            let mut extra = vec![("AWS_CA_BUNDLE", file.as_str())];
            extra.extend_from_slice(endpoint);
            let settings = S3Settings::resolve(&trust_env(&extra)).unwrap();
            let trust = settings
                .trust
                .as_ref()
                .expect("an https endpoint has a source");
            assert_eq!(trust.path(), file);
            assert_eq!(trust.origin(), TrustOrigin::AwsCaBundle);
            assert_eq!(trust.certificates(), std::slice::from_ref(der));
            let shown = format!("{settings:?}");
            assert!(
                shown.contains(&file) && shown.contains("AwsCaBundle"),
                "{shown}"
            );
        }

        // A relative path is made absolute against the working directory, since the readers
        // hand it on for OpenSSL to open later.
        let relative = "relative/ca.pem";
        let absolute = std::env::current_dir().unwrap().join(relative);
        let env = trust_env(&[("AWS_CA_BUNDLE", relative)])
            .with_file(absolute.to_str().unwrap(), Ok(pem));
        let settings = S3Settings::resolve(&env).unwrap();
        assert_eq!(
            settings.trust.as_ref().unwrap().path(),
            absolute.to_str().unwrap()
        );
    }

    #[test]
    fn an_empty_aws_ca_bundle_is_unset() {
        let settings = S3Settings::resolve(&trust_env(&[("AWS_CA_BUNDLE", "")])).unwrap();
        let trust = settings.trust.unwrap();
        assert_eq!(trust.origin(), TrustOrigin::Platform);
        assert_eq!(
            trust.path(),
            PLATFORM_CA_BUNDLES[PLATFORM_CA_BUNDLES.len() - 1]
        );
    }

    #[test]
    fn without_aws_ca_bundle_the_first_platform_bundle_present_is_the_source() {
        let (pem, der) = test_ca();
        for first in 0..PLATFORM_CA_BUNDLES.len() {
            let mut env = trust_env(&[]);
            for later in &PLATFORM_CA_BUNDLES[first..] {
                env = env.with_file(later, Ok(pem));
            }
            let trust = S3Settings::resolve(&env).unwrap().trust.unwrap();
            assert_eq!(trust.path(), PLATFORM_CA_BUNDLES[first]);
            assert_eq!(trust.origin(), TrustOrigin::Platform);
            assert_eq!(trust.certificates(), std::slice::from_ref(der));
        }

        // A bundle that is present but unreadable stops the search; it is not skipped.
        let env =
            trust_env(&[]).with_file(PLATFORM_CA_BUNDLES[0], Err(io::ErrorKind::PermissionDenied));
        let err = format!("{:#}", S3Settings::resolve(&env).unwrap_err());
        assert!(err.contains(PLATFORM_CA_BUNDLES[0]), "{err}");
        assert!(err.contains("AWS_CA_BUNDLE"), "{err}");
    }

    /// Every platform bundle absent.
    fn no_platform_bundle(extra: &[(&str, &str)]) -> FixtureEnv {
        PLATFORM_CA_BUNDLES
            .iter()
            .fold(trust_env(extra), |env, bundle| {
                env.with_file(bundle, Err(io::ErrorKind::NotFound))
            })
    }

    #[test]
    fn no_platform_bundle_is_refused_naming_aws_ca_bundle() {
        for endpoint in HTTPS_ENDPOINTS {
            let err = format!(
                "{:#}",
                S3Settings::resolve(&no_platform_bundle(endpoint)).unwrap_err()
            );
            assert!(err.contains("no platform CA bundle found"), "{err}");
            assert!(err.contains("set AWS_CA_BUNDLE"), "{err}");
            for bundle in PLATFORM_CA_BUNDLES {
                assert!(err.contains(bundle), "{bundle} is not named: {err}");
            }
        }
    }

    /// Every way a CA file can fail to serve, as `(case, contents or read failure, the words
    /// the refusal carries)`.
    fn unusable_bundles() -> Vec<(
        &'static str,
        std::result::Result<String, io::ErrorKind>,
        &'static str,
    )> {
        let (pem, der) = test_ca();
        let key = rcgen::KeyPair::generate().unwrap().serialize_pem();
        vec![
            ("missing", Err(io::ErrorKind::NotFound), "cannot be read"),
            (
                "unreadable",
                Err(io::ErrorKind::PermissionDenied),
                "cannot be read",
            ),
            ("empty", Ok(String::new()), "holds no usable CA certificate"),
            ("a key only", Ok(key), "holds no usable CA certificate"),
            (
                "malformed PEM",
                Ok(format!("{pem}-----BEGIN CERTIFICATE-----\nMIIB\n")),
                "is not valid PEM",
            ),
            (
                "invalid DER",
                Ok(format!(
                    "{pem}{}",
                    pem_block("CERTIFICATE", b"not a certificate")
                )),
                "not a DER certificate",
            ),
            (
                "a DER certificate cut short",
                Ok(format!(
                    "{pem}{}",
                    pem_block("CERTIFICATE", &der[..der.len() - 1])
                )),
                "not a DER certificate",
            ),
            (
                "a TRUSTED CERTIFICATE block",
                Ok(format!("{pem}{}", pem_block("TRUSTED CERTIFICATE", der))),
                "TRUSTED CERTIFICATE",
            ),
            (
                "an intermediate CA and no root",
                Ok(test_intermediate()[0].0.clone()),
                "holds no root CA certificate",
            ),
        ]
    }

    /// The resolver's refusal for each unusable file, named by `AWS_CA_BUNDLE` and as a platform
    /// bundle, over each `https` endpoint.
    fn unusable_bundle_errors() -> Vec<(String, String, &'static str)> {
        let named = "/fixture/named-ca.pem";
        let mut errors = Vec::new();
        for (case, contents, words) in unusable_bundles() {
            let contents = contents.as_deref().map_err(|kind| *kind);
            for endpoint in HTTPS_ENDPOINTS {
                let mut extra = vec![("AWS_CA_BUNDLE", named)];
                extra.extend_from_slice(endpoint);
                let env = trust_env(&extra).with_file(named, contents);
                let err = format!("{:#}", S3Settings::resolve(&env).unwrap_err());
                errors.push((format!("AWS_CA_BUNDLE, {case}"), err, words));
                // An absent platform bundle is skipped, not refused.
                if contents == Err(io::ErrorKind::NotFound) {
                    continue;
                }
                let platform = PLATFORM_CA_BUNDLES[1];
                let env = no_platform_bundle(endpoint).with_file(platform, contents);
                let err = format!("{:#}", S3Settings::resolve(&env).unwrap_err());
                errors.push((format!("{platform}, {case}"), err, words));
            }
        }
        errors
    }

    #[test]
    fn an_unusable_bundle_is_refused_naming_aws_ca_bundle() {
        let errors = unusable_bundle_errors();
        assert_eq!(errors.len(), (9 + 8) * 2);
        for (case, err, words) in errors {
            assert!(err.contains("AWS_CA_BUNDLE"), "{case}: {err}");
            assert!(err.contains(words), "{case}: does not say '{words}': {err}");
            let file = if case.starts_with("AWS_CA_BUNDLE") {
                "/fixture/named-ca.pem"
            } else {
                PLATFORM_CA_BUNDLES[1]
            };
            assert!(err.contains(file), "{case}: does not name {file}: {err}");
            assert!(!err.contains(SECRET), "{case}: {err}");
        }
    }

    #[test]
    fn no_resolver_error_says_the_certificate_is_not_trusted() {
        let mut errors: Vec<String> = unusable_bundle_errors()
            .into_iter()
            .map(|(_, err, _)| err)
            .collect();
        for endpoint in HTTPS_ENDPOINTS {
            errors.push(format!(
                "{:#}",
                S3Settings::resolve(&no_platform_bundle(endpoint)).unwrap_err()
            ));
        }
        assert!(errors.len() > 30);
        for err in errors {
            assert!(!err.contains("TLS certificate is not trusted"), "{err}");
            assert!(!err.contains("not trusted"), "{err}");
        }
    }

    #[test]
    fn a_certificate_webpki_rejects_is_dropped_and_the_rest_kept() {
        let (refused_pem, refused_der) = make_ca("xdu refused CA", true);
        let (kept_pem, kept_der) = make_ca("xdu kept CA", false);
        // The premise: the refused certificate is DER that webpki will not take as an anchor.
        assert!(is_certificate_der(&refused_der));
        assert!(
            rustls::RootCertStore::empty()
                .add(refused_der.clone())
                .is_err()
        );

        let both = format!("{refused_pem}{kept_pem}");
        assert_eq!(bundle_certificates(&both).unwrap(), vec![kept_der.clone()]);
        let env = trust_env(&[("AWS_CA_BUNDLE", "/fixture/mixed.pem")])
            .with_file("/fixture/mixed.pem", Ok(&both));
        let trust = S3Settings::resolve(&env).unwrap().trust.unwrap();
        assert_eq!(trust.certificates(), std::slice::from_ref(&kept_der));

        // Alone, it leaves nothing to trust.
        let err = bundle_certificates(&refused_pem).unwrap_err();
        assert!(err.contains("holds no usable CA certificate"), "{err}");
    }

    #[test]
    fn a_bundle_without_a_root_is_refused_naming_aws_ca_bundle() {
        let [(intermediate_pem, intermediate_der), (leaf_pem, _)] = test_intermediate();
        let (root_pem, root_der) = test_ca();
        let named = "/fixture/no-root.pem";
        for rootless in [
            intermediate_pem.clone(),
            format!("{leaf_pem}{intermediate_pem}"),
        ] {
            for endpoint in HTTPS_ENDPOINTS {
                let mut extra = vec![("AWS_CA_BUNDLE", named)];
                extra.extend_from_slice(endpoint);
                let env = trust_env(&extra).with_file(named, Ok(&rootless));
                let err = format!("{:#}", S3Settings::resolve(&env).unwrap_err());
                assert!(
                    err.contains("AWS_CA_BUNDLE names /fixture/no-root.pem"),
                    "{err}"
                );
                assert!(err.contains("holds no root CA certificate"), "{err}");
                assert!(!err.contains("not trusted"), "{err}");

                let platform = PLATFORM_CA_BUNDLES[0];
                let env = trust_env(endpoint).with_file(platform, Ok(&rootless));
                let err = format!("{:#}", S3Settings::resolve(&env).unwrap_err());
                assert!(err.contains(platform), "{err}");
                assert!(err.contains("holds no root CA certificate"), "{err}");
                assert!(err.contains("set AWS_CA_BUNDLE"), "{err}");
                assert!(!err.contains("not trusted"), "{err}");
            }
        }

        // With its root beside it, the intermediate serves, and both are kept in file order.
        let both = format!("{intermediate_pem}{root_pem}");
        let env = trust_env(&[("AWS_CA_BUNDLE", named)]).with_file(named, Ok(&both));
        let trust = S3Settings::resolve(&env).unwrap().trust.unwrap();
        assert_eq!(
            trust.certificates(),
            &[intermediate_der.clone(), root_der.clone()]
        );
    }

    #[test]
    fn a_root_is_a_certificate_its_own_key_signed() {
        use rcgen::{CertificateParams, DnType, Issuer, KeyPair};
        let [(_, intermediate), (_, leaf)] = test_intermediate();
        assert!(is_self_signed_root(&test_ca().1));
        assert!(is_self_signed_root(&make_ca("xdu another root", false).1));
        assert!(!is_self_signed_root(intermediate));
        assert!(!is_self_signed_root(leaf));

        // Issued under its own name, but signed by another key: not a root.
        let name = "xdu look-alike root";
        let mut signer_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        signer_params
            .distinguished_name
            .push(DnType::CommonName, name);
        let signer = Issuer::new(signer_params, KeyPair::generate().unwrap());
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.distinguished_name.push(DnType::CommonName, name);
        let look_alike = params
            .signed_by(&KeyPair::generate().unwrap(), &signer)
            .unwrap();
        let parts = CertificateParts::parse(look_alike.der()).unwrap();
        assert_eq!(parts.issuer, parts.subject);
        assert!(!is_self_signed_root(look_alike.der()));

        // A root with one bit of its signature flipped: its own key no longer verifies it.
        let mut flipped = test_ca().1.to_vec();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        assert!(is_certificate_der(&flipped));
        assert!(!is_self_signed_root(&flipped));

        for garbage in [&b""[..], b"not a certificate", &test_ca().1[..40]] {
            assert!(!is_self_signed_root(garbage));
        }
    }

    #[test]
    fn every_platform_bundle_on_this_host_holds_a_root() {
        // The root rule must not refuse the bundles hosts ship. A host with none cannot reach an
        // https endpoint at all, so it fails here rather than passing on nothing.
        let present: Vec<&str> = PLATFORM_CA_BUNDLES
            .iter()
            .copied()
            .filter(|bundle| Path::new(bundle).is_file())
            .collect();
        assert!(
            !present.is_empty(),
            "no platform CA bundle on this host (looked for {PLATFORM_CA_BUNDLES:?})"
        );
        for bundle in present {
            let text = fs::read_to_string(bundle).unwrap();
            let kept =
                bundle_certificates(&text).unwrap_or_else(|reason| panic!("{bundle} {reason}"));
            let roots = kept.iter().filter(|c| is_self_signed_root(c)).count();
            assert!(
                roots > 0,
                "{bundle}: no root among {} certificates",
                kept.len()
            );
        }
    }

    #[test]
    fn a_plain_http_endpoint_resolves_no_trust_source() {
        let http = ("AWS_ENDPOINT_URL", "http://127.0.0.1:9000");
        // Neither an unusable AWS_CA_BUNDLE nor a host without a platform bundle stops it.
        for env in [
            trust_env(&[http, ("AWS_CA_BUNDLE", "/fixture/missing.pem")])
                .with_file("/fixture/missing.pem", Err(io::ErrorKind::NotFound)),
            no_platform_bundle(&[http]),
            trust_env(&[http]),
        ] {
            let settings = S3Settings::resolve(&env).unwrap();
            assert!(settings.trust.is_none(), "{settings:?}");
            assert!(!settings.use_ssl());
        }
        let https = ("AWS_ENDPOINT_URL", "https://127.0.0.1:9000");
        assert!(S3Settings::resolve(&no_platform_bundle(&[https])).is_err());
    }

    #[test]
    fn the_tls_statements_are_fixed_text_and_bind_only_the_path() {
        let dir = TempDir::new().unwrap();
        let quoted = dir.path().join("it's; DROP");
        fs::create_dir(&quoted).unwrap();
        let (pem, _) = test_ca();
        let files = [bundle_file(&dir, "ca.pem", pem), {
            let file = quoted.join("ca.pem");
            fs::write(&file, pem).unwrap();
            file.to_str().unwrap().to_string()
        }];
        let plain =
            S3Settings::resolve(&trust_env(&[("AWS_ENDPOINT_URL", "http://127.0.0.1:9000")]))
                .unwrap()
                .duckdb_tls_statements();
        // Over plain HTTP: the three fixed statements, binding nothing.
        assert_eq!(plain.len(), 3);
        for (sql, params) in &plain {
            assert!(params.is_empty(), "{sql}");
            assert!(sql.starts_with("SET ") && !sql.contains(';'), "{sql}");
        }
        for file in &files {
            for endpoint in HTTPS_ENDPOINTS {
                let mut extra = vec![("AWS_CA_BUNDLE", file.as_str())];
                extra.extend_from_slice(endpoint);
                let statements = S3Settings::resolve(&trust_env(&extra))
                    .unwrap()
                    .duckdb_tls_statements();
                // The same text whatever the path; the fixed three first, then the file.
                let texts: Vec<&str> = statements.iter().map(|(sql, _)| *sql).collect();
                let plain_texts: Vec<&str> = plain.iter().map(|(sql, _)| *sql).collect();
                assert_eq!(texts[..3], plain_texts[..]);
                assert_eq!(texts.len(), 4);
                let (sql, params) = &statements[3];
                assert_eq!(sql.matches('?').count(), 1, "{sql}");
                assert_eq!(params, &vec![Value::Text(file.clone())]);
                for (sql, _) in &statements {
                    assert!(!sql.contains(file.as_str()), "{sql}");
                    assert!(!sql.contains("it's"), "{sql}");
                }
            }
        }
    }
}
