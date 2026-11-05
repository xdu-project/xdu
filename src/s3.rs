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

use crate::COMPLETION_MARKER;
use crate::crawl::{IndexSink, TopEntry, chunk_final_name};

/// The scheme that marks an index argument as an S3 locator. Lowercase only: `S3://x` is a
/// relative local path, as it always was.
pub const S3_SCHEME: &str = "s3://";

/// Characters DuckDB's glob expansion treats as patterns. A bucket or prefix holding one would
/// make the index glob match keys outside the index, so the parser refuses them.
const GLOB_METACHARACTERS: &[char] = &['*', '?', '[', ']', '{', '}'];

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
    /// Refused: bytes that are not UTF-8, an empty bucket, a `user:secret@` form (credentials
    /// never travel in a locator, and the error does not echo it), and glob metacharacters in
    /// the bucket or prefix.
    pub fn parse(raw: &OsStr) -> Result<Self> {
        let Some(text) = raw.to_str() else {
            bail!("S3 locator is not valid UTF-8: {}", raw.to_string_lossy());
        };
        let Some(rest) = text.strip_prefix(S3_SCHEME) else {
            bail!("{text} is not an S3 locator; expected s3://BUCKET/PREFIX");
        };
        let (bucket, prefix) = rest.split_once('/').unwrap_or((rest, ""));

        // Checked before anything that would print the locator: the part before `@` may be
        // a secret.
        if bucket.contains('@') {
            bail!(
                "an S3 locator must not carry credentials before the bucket; give them through \
                 the AWS_* variables or the shared credentials file"
            );
        }
        if bucket.is_empty() {
            bail!("S3 locator {text} names no bucket; expected s3://BUCKET/PREFIX");
        }
        if let Some(c) = text[S3_SCHEME.len()..]
            .chars()
            .find(|c| GLOB_METACHARACTERS.contains(c))
        {
            bail!(
                "S3 locator {text} contains the glob metacharacter '{c}', which would let the \
                 index glob match keys outside the index"
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

/// Everything an S3 client needs, resolved once from the AWS variables and shared files.
///
/// `Debug` prints neither the secret nor the session token.
#[derive(Clone)]
pub struct S3Settings {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub region: String,
    /// `None` is AWS's own endpoint.
    pub endpoint: Option<S3Endpoint>,
    /// Path-style addressing (`host/bucket/key`). On whenever an endpoint override is set,
    /// since S3-compatible stores rarely serve virtual-hosted names; off for AWS itself.
    pub path_style: bool,
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
            .finish()
    }
}

/// The name of the one secret a reader connection holds.
const DUCKDB_SECRET_NAME: &str = "xdu_s3";

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
    /// It fails closed: half-set environment credentials, a profile holding only one half of
    /// a key pair, a profile named by `AWS_PROFILE` that neither file holds, no credentials at
    /// all, and an endpoint that is not a bare `http(s)://host[:port]` are errors.
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

        let region = match env_text(env, "AWS_REGION")? {
            Some(region) => region,
            None => match env_text(env, "AWS_DEFAULT_REGION")? {
                Some(region) => region,
                None => config_section
                    .and_then(|s| s.value("region"))
                    .map(str::to_string)
                    .unwrap_or_else(|| DEFAULT_REGION.to_string()),
            },
        };

        let endpoint = resolve_endpoint(
            env,
            &config,
            config_section,
            config_file.as_deref(),
            &profile,
        )?;
        let path_style = endpoint.is_some();

        Ok(S3Settings {
            access_key_id,
            secret_access_key,
            session_token,
            region,
            endpoint,
            path_style,
        })
    }

    /// The `CREATE SECRET` a reader connection runs, and the values it binds.
    ///
    /// Every value is a bound parameter; the text is fixed but for which optional clauses
    /// appear, chosen by presence. A secret held this way shows as `redacted` in DuckDB's
    /// `duckdb_secrets()`, appears in no `duckdb_settings()` row, and, matching every `s3://`
    /// path, outranks whatever `httpfs` read from `AWS_*` or `DUCKDB_S3_*` when it loaded.
    pub fn duckdb_secret_statement(&self) -> (String, Vec<Value>) {
        let mut sql = format!(
            "CREATE TEMPORARY SECRET {DUCKDB_SECRET_NAME} (TYPE s3, PROVIDER config, \
             KEY_ID ?, SECRET ?, REGION ?, URL_STYLE ?, USE_SSL ?"
        );
        let url_style = if self.path_style { "path" } else { "vhost" };
        let use_ssl = self.endpoint.as_ref().is_none_or(S3Endpoint::use_ssl);
        let mut params = vec![
            Value::Text(self.access_key_id.clone()),
            Value::Text(self.secret_access_key.clone()),
            Value::Text(self.region.clone()),
            Value::Text(url_style.to_string()),
            Value::Boolean(use_ssl),
        ];
        if let Some(token) = &self.session_token {
            sql.push_str(", SESSION_TOKEN ?");
            params.push(Value::Text(token.clone()));
        }
        if let Some(endpoint) = &self.endpoint {
            sql.push_str(", ENDPOINT ?");
            params.push(Value::Text(endpoint.authority().to_string()));
        }
        sql.push(')');
        (sql, params)
    }
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
/// Sections are `[name]`; keys are case-folded and values trimmed; whole-line `#` and `;`
/// comments are skipped. A key with an empty value opens a nested block whose indented
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
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let name = trimmed[1..trimmed.len() - 1]
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                ini.sections.entry(name.clone()).or_default();
                current = Some(name);
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
#[derive(Debug)]
struct DirectConnector;

impl HttpConnector for DirectConnector {
    fn connect(&self, _options: &ClientOptions) -> object_store::Result<HttpClient> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| object_store::Error::Generic {
                store: "S3",
                source: Box::new(e),
            })?;
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
}

impl S3Sink {
    /// A sink for `locator`, with the client built from `settings` alone: the static key pair,
    /// region and endpoint the resolver found, never the client's own environment or metadata
    /// lookups. Nothing is sent until `preflight`.
    pub fn connect(locator: &S3Locator, settings: &S3Settings) -> Result<S3Sink> {
        install_crypto_provider();
        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(locator.bucket())
            .with_region(&settings.region)
            .with_access_key_id(&settings.access_key_id)
            .with_secret_access_key(&settings.secret_access_key)
            .with_virtual_hosted_style_request(!settings.path_style)
            .with_retry(retry_config())
            .with_disable_bulk_delete(true)
            .with_http_connector(DirectConnector);
        if let Some(token) = &settings.session_token {
            builder = builder.with_token(token);
        }
        if let Some(endpoint) = &settings.endpoint {
            builder = builder
                .with_endpoint(endpoint.url())
                .with_allow_http(!endpoint.use_ssl());
        }
        let store = builder
            .build()
            .with_context(|| format!("cannot configure an S3 client for {locator}"))?;
        S3Sink::with_store(Arc::new(store), locator)
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

    /// Name the cause of a failed request: the endpoint unreachable, the credentials refused,
    /// the bucket missing, or the store's own words. No credential is ever part of the text.
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
        match status {
            Some(401 | 403) => anyhow!(
                "the S3 endpoint refused the credentials while {what} for {index}; check \
                 AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or the profile AWS_PROFILE names, \
                 and that they may write there ({err})"
            ),
            Some(404) => anyhow!(
                "bucket '{}' does not exist at the S3 endpoint (HTTP 404 while {what} for {index})",
                index.bucket()
            ),
            _ if is_unreachable(&err) => anyhow!(
                "cannot reach the S3 endpoint while {what} for {index}: no connection could be \
                 established; check AWS_ENDPOINT_URL_S3, AWS_ENDPOINT_URL or the profile's \
                 endpoint_url ({err})"
            ),
            _ => anyhow!("S3 request failed while {what} for {index}: {err}"),
        }
    }
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
            "s3://buck*et/p",
            "s3://bucket/p]",
        ] {
            let err = locator(raw).unwrap_err().to_string();
            assert!(err.contains("glob metacharacter"), "{raw}: {err}");
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

    /// Variables from a map, files from disk.
    struct FixtureEnv(HashMap<String, OsString>);

    impl FixtureEnv {
        fn new(vars: &[(&str, &str)]) -> Self {
            FixtureEnv(
                vars.iter()
                    .map(|(k, v)| (k.to_string(), OsString::from(v)))
                    .collect(),
            )
        }
    }

    impl EnvSource for FixtureEnv {
        fn var(&self, name: &str) -> Option<OsString> {
            self.0.get(name).cloned()
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
            assert_eq!(sql.contains("SESSION_TOKEN ?"), token, "{sql}");
            assert_eq!(sql.contains("ENDPOINT ?"), endpoint, "{sql}");
        }
    }

    #[test]
    fn the_secret_statement_addresses_by_endpoint() {
        let (_, aws) = settings(false, false).duckdb_secret_statement();
        assert_eq!(aws[3], Value::Text("vhost".to_string()));
        assert_eq!(aws[4], Value::Boolean(true));
        let (_, local) = settings(false, true).duckdb_secret_statement();
        assert_eq!(local[3], Value::Text("path".to_string()));
        assert_eq!(local[4], Value::Boolean(false));
        assert_eq!(
            local.last(),
            Some(&Value::Text("127.0.0.1:9000".to_string()))
        );
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
}
