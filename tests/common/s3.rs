//! The loopback S3 endpoint, the counting proxy and the hermetic child environment the S3 tests
//! share.
//!
//! `TestS3::start()` runs an s3s-fs endpoint inside the test process on `127.0.0.1:0`, with
//! SigV4 checked against one fixed key pair, and a counting proxy in front of it. Clients are
//! handed only the proxy's address, so a request that does not pass the meter cannot reach the
//! data. The endpoint's root directory holds each bucket as a plain directory, so a test seeds
//! an index by copying a local one in and measures stored bytes by walking it.
//!
//! The server lives in a runtime owned by the `TestS3`; dropping the value stops it, so no test
//! can leave one running. Starting panics when the pinned `httpfs` is missing or the bind is
//! refused, whatever else is set: a wire test that cannot run must fail, never report `ok`.
//! The `#[ignore]` on each wire test is what keeps a machine without the extension quiet.
//!
//! `TestS3::tls_front` puts a TLS terminator before the proxy, with certificates `TestPki`
//! makes per run, so the certificate tests reach the same endpoint, through the same meter, over
//! `https`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnBuilder;
use s3s::auth::SimpleAuth;
use s3s::dto::{
    DeleteObjectInput, DeleteObjectOutput, DeleteObjectsInput, DeleteObjectsOutput, EncodingType,
    GetObjectInput, GetObjectOutput, HeadBucketInput, HeadBucketOutput, HeadObjectInput,
    HeadObjectOutput, ListBucketsInput, ListBucketsOutput, ListObjectsInput, ListObjectsOutput,
    ListObjectsV2Input, ListObjectsV2Output, PutObjectInput, PutObjectOutput,
};
use s3s::service::S3ServiceBuilder;
use s3s::{S3, S3Request, S3Response, S3Result, s3_error};
use tempfile::TempDir;
use tokio_rustls::rustls;

use xdu::s3::{EnvSource, S3Settings};

use super::binary_path;

/// Names the directory holding the pinned `httpfs` for the S3 tests. Deliberately not
/// `XDU_EXTENSION_DIR`, which the binaries read: each S3 test passes the directory to its
/// children itself, so a local-index test can never pick the extension up by accident.
pub const EXTENSION_DIR_VAR: &str = "XDU_TEST_EXTENSION_DIR";

pub const BUCKET: &str = "xdu-test";
pub const ACCESS_KEY: &str = "xdu-test-access-key";
/// Distinctive, so a leak into any output is found by a plain substring search.
pub const SECRET_KEY: &str = "xdu-test-secret-Qm7Vr2Kx9Pw4";
/// A second secret for the rejected-credential cases; it must not leak either.
pub const WRONG_SECRET_KEY: &str = "xdu-wrong-secret-Hd3Lz8Tn5Ry1";
pub const SESSION_TOKEN: &str = "xdu-test-session-token-Jf6Wb1Sc4Ne8";
/// A second identity the endpoint accepts, allowed to read but refused every write.
pub const READ_ONLY_ACCESS_KEY: &str = "xdu-test-read-only-key";
pub const READ_ONLY_SECRET_KEY: &str = "xdu-read-only-secret-Wc5Gt7Mb2Xq9";
/// A partition the endpoint refuses every chunk PUT under, whoever signs it: a write failure
/// injected in the middle of a run.
pub const REFUSED_PARTITION: &str = "refuse-writes";

/// The working directory the test process runs in, read once. Nothing in the tests changes
/// it, so it is where the process started, and a relative `XDU_TEST_EXTENSION_DIR` resolves
/// against it as the shell that set it would.
static START_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The directory holding the pinned `httpfs`, as an absolute path. Panics when it is unset or
/// lacks the file.
///
/// Absolute because it is handed on: every spawned binary gets it as `XDU_EXTENSION_DIR` and
/// runs in an empty working directory of its own, where a relative path names nothing.
pub fn test_extension_dir() -> PathBuf {
    let Some(dir) = std::env::var_os(EXTENSION_DIR_VAR).filter(|d| !d.is_empty()) else {
        panic!(
            "{EXTENSION_DIR_VAR} is unset; fetch the pinned httpfs with \
             packaging/extensions/fetch.sh and point {EXTENSION_DIR_VAR} at it"
        );
    };
    let start = START_DIR
        .get_or_init(|| std::env::current_dir().expect("the test's working directory is readable"));
    let dir = start.join(dir);
    let file = dir.join("httpfs.duckdb_extension");
    assert!(
        file.is_file(),
        "{EXTENSION_DIR_VAR} names {}, which holds no httpfs.duckdb_extension",
        dir.display()
    );
    dir
}

/// Variables from a map; files from disk.
pub struct MapEnv(pub HashMap<String, OsString>);

impl MapEnv {
    pub fn new(vars: &[(&str, &str)]) -> Self {
        MapEnv(
            vars.iter()
                .map(|(k, v)| (k.to_string(), OsString::from(v)))
                .collect(),
        )
    }
}

impl EnvSource for MapEnv {
    fn var(&self, name: &str) -> Option<OsString> {
        self.0.get(name).cloned()
    }
}

/// Settings resolved through the library's resolver, as a binary would resolve them from its
/// environment, with the endpoint set to `endpoint_url`.
pub fn resolve_settings(endpoint_url: &str, secret: &str, token: Option<&str>) -> S3Settings {
    let mut vars = vec![
        ("AWS_ACCESS_KEY_ID", ACCESS_KEY),
        ("AWS_SECRET_ACCESS_KEY", secret),
        ("AWS_ENDPOINT_URL", endpoint_url),
    ];
    if let Some(token) = token {
        vars.push(("AWS_SESSION_TOKEN", token));
    }
    S3Settings::resolve(&MapEnv::new(&vars)).expect("fixture settings resolve")
}

/// s3s-fs with ListObjectsV2's `encoding-type=url` honoured, writes refused for the read-only
/// identity and under `REFUSED_PARTITION`, and every other operation the tests use passed
/// through unchanged.
///
/// Asked for `encoding-type=url`, s3s-fs echoes `EncodingType` but sends each key raw, so the
/// XML escapes a `'` as `&apos;`. S3 percent-encodes the keys it lists when asked, and httpfs,
/// which always asks, decodes percent escapes and not XML entities: without this a key holding
/// `'` would list as a key that does not exist, on this endpoint only.
struct UrlEncodedListing(s3s_fs::FileSystem);

/// Refuse a write signed by the read-only identity, as S3 refuses one its policy denies.
fn refuse_read_only<T>(req: &S3Request<T>) -> S3Result<()> {
    match &req.credentials {
        Some(c) if c.access_key == READ_ONLY_ACCESS_KEY => Err(s3_error!(AccessDenied)),
        _ => Ok(()),
    }
}

/// Refuse a PUT, after reading its body to the end. A server that answers with the body still
/// unread closes on unread data, and the reset that follows can reach the proxy before the
/// refusal does.
async fn refuse_put(mut req: S3Request<PutObjectInput>) -> S3Result<S3Response<PutObjectOutput>> {
    use futures::StreamExt;
    if let Some(body) = req.input.body.as_mut() {
        while let Some(chunk) = body.next().await {
            if chunk.is_err() {
                break;
            }
        }
    }
    Err(s3_error!(AccessDenied))
}

/// Percent-encode everything but unreserved characters and `/`, as S3 encodes a listed key.
fn url_encode_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for byte in key.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[async_trait::async_trait]
impl S3 for UrlEncodedListing {
    async fn list_objects_v2(
        &self,
        req: S3Request<ListObjectsV2Input>,
    ) -> S3Result<S3Response<ListObjectsV2Output>> {
        let encode = req
            .input
            .encoding_type
            .as_ref()
            .is_some_and(|e| e.as_str() == EncodingType::URL);
        let mut response = self.0.list_objects_v2(req).await?;
        if encode {
            let output = &mut response.output;
            for field in [
                &mut output.prefix,
                &mut output.start_after,
                &mut output.delimiter,
            ] {
                if let Some(value) = field.as_mut() {
                    *value = url_encode_key(value);
                }
            }
            for object in output.contents.iter_mut().flatten() {
                if let Some(key) = object.key.as_mut() {
                    *key = url_encode_key(key);
                }
            }
            for common in output.common_prefixes.iter_mut().flatten() {
                if let Some(prefix) = common.prefix.as_mut() {
                    *prefix = url_encode_key(prefix);
                }
            }
        }
        Ok(response)
    }

    async fn list_objects(
        &self,
        req: S3Request<ListObjectsInput>,
    ) -> S3Result<S3Response<ListObjectsOutput>> {
        self.0.list_objects(req).await
    }

    async fn list_buckets(
        &self,
        req: S3Request<ListBucketsInput>,
    ) -> S3Result<S3Response<ListBucketsOutput>> {
        self.0.list_buckets(req).await
    }

    async fn head_bucket(
        &self,
        req: S3Request<HeadBucketInput>,
    ) -> S3Result<S3Response<HeadBucketOutput>> {
        self.0.head_bucket(req).await
    }

    async fn head_object(
        &self,
        req: S3Request<HeadObjectInput>,
    ) -> S3Result<S3Response<HeadObjectOutput>> {
        self.0.head_object(req).await
    }

    async fn get_object(
        &self,
        req: S3Request<GetObjectInput>,
    ) -> S3Result<S3Response<GetObjectOutput>> {
        self.0.get_object(req).await
    }

    async fn put_object(
        &self,
        req: S3Request<PutObjectInput>,
    ) -> S3Result<S3Response<PutObjectOutput>> {
        let refused_partition = req
            .input
            .key
            .split('/')
            .any(|segment| segment == REFUSED_PARTITION);
        if refused_partition || refuse_read_only(&req).is_err() {
            return refuse_put(req).await;
        }
        self.0.put_object(req).await
    }

    async fn delete_object(
        &self,
        req: S3Request<DeleteObjectInput>,
    ) -> S3Result<S3Response<DeleteObjectOutput>> {
        refuse_read_only(&req)?;
        self.0.delete_object(req).await
    }

    async fn delete_objects(
        &self,
        req: S3Request<DeleteObjectsInput>,
    ) -> S3Result<S3Response<DeleteObjectsOutput>> {
        refuse_read_only(&req)?;
        self.0.delete_objects(req).await
    }
}

/// A loopback S3 endpoint behind a counting proxy.
pub struct TestS3 {
    proxy: CountingProxy,
    /// Connections the endpoint itself accepted. Only the proxy is told its address.
    endpoint_connections: Arc<AtomicUsize>,
    // Dropped after the proxy, so the proxy never forwards to a stopped server mid-test.
    runtime: Option<tokio::runtime::Runtime>,
    root: TempDir,
    extension_dir: PathBuf,
}

impl TestS3 {
    pub fn start() -> TestS3 {
        let extension_dir = test_extension_dir();
        let root = TempDir::new().expect("endpoint root directory");
        fs::create_dir(root.path().join(BUCKET)).expect("bucket directory");

        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("cannot bind the loopback S3 endpoint: {e}"));
        listener
            .set_nonblocking(true)
            .expect("nonblocking endpoint listener");
        let endpoint = listener.local_addr().expect("endpoint address");

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("endpoint runtime");
        let filesystem = s3s_fs::FileSystem::new(root.path()).expect("s3s-fs root");
        let mut builder = S3ServiceBuilder::new(UrlEncodedListing(filesystem));
        let mut auth = SimpleAuth::from_single(ACCESS_KEY, SECRET_KEY);
        auth.register(
            READ_ONLY_ACCESS_KEY.to_string(),
            READ_ONLY_SECRET_KEY.into(),
        );
        builder.set_auth(auth);
        let service = builder.build();
        let endpoint_connections = Arc::new(AtomicUsize::new(0));
        let accepted = Arc::clone(&endpoint_connections);
        runtime.spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("endpoint listener");
            let http = ConnBuilder::new(TokioExecutor::new());
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    continue;
                };
                accepted.fetch_add(1, Ordering::SeqCst);
                let conn = http
                    .serve_connection(TokioIo::new(socket), service.clone())
                    .into_owned();
                tokio::spawn(conn);
            }
        });

        let proxy = CountingProxy::start(endpoint);
        TestS3 {
            proxy,
            endpoint_connections,
            runtime: Some(runtime),
            root,
            extension_dir,
        }
    }

    pub fn extension_dir(&self) -> &Path {
        &self.extension_dir
    }

    /// What every client is given as its endpoint.
    pub fn proxy_url(&self) -> String {
        format!("http://{}", self.proxy.addr)
    }

    /// Settings for this endpoint, through the proxy, with the accepted key pair.
    pub fn settings(&self) -> S3Settings {
        resolve_settings(&self.proxy_url(), SECRET_KEY, None)
    }

    pub fn url(&self, key: &str) -> String {
        format!("s3://{BUCKET}/{key}")
    }

    /// `xdu-find` in `hermetic`, reading through this endpoint's proxy with the accepted key
    /// pair from the environment and the pinned `httpfs`. The caller adds `-i` and the query.
    pub fn find_command(&self, hermetic: &Hermetic) -> Command {
        let mut cmd = hermetic.command("xdu-find");
        cmd.env("XDU_EXTENSION_DIR", self.extension_dir())
            .env("AWS_ACCESS_KEY_ID", ACCESS_KEY)
            .env("AWS_SECRET_ACCESS_KEY", SECRET_KEY)
            .env("AWS_ENDPOINT_URL", self.proxy_url());
        cmd
    }

    /// `xdu-view` in `hermetic`, set up as `find_command` sets up `xdu-find`. Only its refusals
    /// are drivable here: an accepted index goes on to need a terminal.
    pub fn view_command(&self, hermetic: &Hermetic) -> Command {
        let mut cmd = hermetic.command("xdu-view");
        cmd.env("XDU_EXTENSION_DIR", self.extension_dir())
            .env("AWS_ACCESS_KEY_ID", ACCESS_KEY)
            .env("AWS_SECRET_ACCESS_KEY", SECRET_KEY)
            .env("AWS_ENDPOINT_URL", self.proxy_url());
        cmd
    }

    /// `xdu` in `hermetic`, writing through this endpoint's proxy with the accepted key pair
    /// from the environment. The writer needs no extension, so none is offered. The caller adds
    /// the source and `-o`.
    pub fn write_command(&self, hermetic: &Hermetic) -> Command {
        let mut cmd = hermetic.command("xdu");
        cmd.env("AWS_ACCESS_KEY_ID", ACCESS_KEY)
            .env("AWS_SECRET_ACCESS_KEY", SECRET_KEY)
            .env("AWS_ENDPOINT_URL", self.proxy_url());
        cmd
    }

    fn bucket_dir(&self) -> PathBuf {
        self.root.path().join(BUCKET)
    }

    /// The directory the endpoint stores the objects under `prefix` in.
    pub fn object_dir(&self, prefix: &str) -> PathBuf {
        self.bucket_dir().join(prefix)
    }

    /// Every key under `prefix/`, relative to it, read from the endpoint's root.
    pub fn keys(&self, prefix: &str) -> BTreeSet<String> {
        let base = self.object_dir(prefix);
        if !base.exists() {
            return BTreeSet::new();
        }
        super::list_files_recursive(&base)
            .into_iter()
            .map(|f| f.strip_prefix(&base).unwrap().to_str().unwrap().to_string())
            .collect()
    }

    /// Every stored object under `prefix/` with its bytes, keyed relative to the prefix.
    pub fn snapshot(&self, prefix: &str) -> BTreeMap<String, Vec<u8>> {
        self.keys(prefix)
            .into_iter()
            .map(|k| {
                let bytes = fs::read(self.object_dir(prefix).join(&k)).unwrap();
                (k, bytes)
            })
            .collect()
    }

    /// The stored bytes of an object, read from the endpoint's root.
    pub fn object(&self, key: &str) -> Vec<u8> {
        fs::read(self.bucket_dir().join(key)).unwrap()
    }

    pub fn seed_object(&self, key: &str, bytes: &[u8]) {
        let file = self.bucket_dir().join(key);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }

    /// Copy a local index under `<bucket>/<prefix>/`, every file at its relative path.
    pub fn seed_index(&self, local_index: &Path, prefix: &str) {
        let files = super::list_files_recursive(local_index);
        assert!(!files.is_empty(), "{} is empty", local_index.display());
        for file in files {
            let rel = file.strip_prefix(local_index).unwrap();
            let key = format!("{prefix}/{}", rel.to_str().unwrap());
            self.seed_object(&key, &fs::read(&file).unwrap());
        }
    }

    /// Stored size of every `*.parquet` key under `prefix/`, read from the endpoint's root.
    pub fn parquet_sizes(&self, prefix: &str) -> BTreeMap<String, u64> {
        let base = self.bucket_dir();
        super::list_files_recursive(&base.join(prefix))
            .into_iter()
            .filter(|f| f.extension().is_some_and(|e| e == "parquet"))
            .map(|f| {
                let key = f.strip_prefix(&base).unwrap().to_str().unwrap().to_string();
                let size = fs::metadata(&f).unwrap().len();
                (key, size)
            })
            .collect()
    }

    /// Every request the proxy has relayed so far, in arrival order.
    pub fn requests(&self) -> Vec<ProxyRecord> {
        self.proxy.records()
    }

    /// The proxy hit no error of its own, every connection that carried bytes carried exactly
    /// one recorded request, and the endpoint saw no connection but the proxy's one per
    /// recorded request. A connection that carried nothing is not a request.
    pub fn assert_proxy_clean(&self) {
        self.proxy.assert_clean(&self.endpoint_connections);
    }

    /// The proxy's address, for a test of the proxy itself.
    pub fn proxy_addr(&self) -> SocketAddr {
        self.proxy.addr
    }

    /// No GET of a `*.parquet` key covered the whole object. A GET without a `Range`, an
    /// open-ended `bytes=a-` or suffix `bytes=-n` range, a `200`, a missing `Content-Range`, or
    /// a bounded range as long as the object all count as whole.
    pub fn assert_no_whole_parquet_get(&self) {
        let base = self.bucket_dir();
        for record in self.requests() {
            if record.method != "GET" || !record.key().ends_with(".parquet") {
                continue;
            }
            let size = fs::metadata(base.join(record.key()))
                .unwrap_or_else(|e| panic!("GET of {}, not in the bucket: {e}", record.key()))
                .len();
            let Some((first, last)) = record.bounded_range() else {
                panic!("whole-object GET of {}: {record:?}", record.key());
            };
            assert!(
                record.status == 206 && record.content_range.is_some(),
                "GET of {} was not served as a partial response: {record:?}",
                record.key()
            );
            assert!(
                last >= first && last - first + 1 < size,
                "GET of {} covered the whole {size}-byte object: {record:?}",
                record.key()
            );
        }
    }
}

impl Drop for TestS3 {
    fn drop(&mut self) {
        self.proxy.stop();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(5));
        }
    }
}

/// What a TLS endpoint shows its clients: one certificate and its key.
#[derive(Clone)]
pub struct FrontIdentity {
    certificate: rustls::pki_types::CertificateDer<'static>,
    key: Vec<u8>,
}

/// Certificates made for one test run with `rcgen`: a test CA, an unrelated CA, and the
/// identities a TLS front can show.
///
/// The self-signed certificate is an end-entity one. A self-signed CA certificate shown as a
/// server's would be refused as a CA used as an end entity, which is a different rule from the
/// one an untrusted issuer breaks. Each CA's PEM file sits in a directory of its own, apart from
/// every `Hermetic` directory, so a child that wrote beside it would still be caught.
pub struct TestPki {
    dir: TempDir,
    /// A leaf the test CA signed for `IP:127.0.0.1`.
    pub leaf: FrontIdentity,
    /// A leaf the test CA signed for `DNS:wrong.example` only.
    pub wrong_name: FrontIdentity,
    /// A self-signed end-entity certificate for `IP:127.0.0.1`.
    pub self_signed: FrontIdentity,
}

impl TestPki {
    pub fn new() -> TestPki {
        use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};

        let make_ca = |name: &str| {
            let key = KeyPair::generate().expect("CA key");
            let mut params = CertificateParams::new(Vec::<String>::new()).expect("CA params");
            params.distinguished_name.push(DnType::CommonName, name);
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            let certificate = params.self_signed(&key).expect("CA certificate");
            (certificate, Issuer::new(params, key))
        };
        let identity = |names: &[&str], issuer: Option<&Issuer<'_, KeyPair>>| {
            let key = KeyPair::generate().expect("leaf key");
            let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
            let mut params = CertificateParams::new(names).expect("leaf params");
            params
                .distinguished_name
                .push(DnType::CommonName, "xdu test endpoint");
            let certificate = match issuer {
                Some(issuer) => params.signed_by(&key, issuer),
                None => params.self_signed(&key),
            }
            .expect("leaf certificate");
            FrontIdentity {
                certificate: certificate.der().clone(),
                key: key.serialize_der(),
            }
        };

        let (ca_certificate, ca) = make_ca("xdu test CA");
        let (other_certificate, _) = make_ca("xdu unrelated CA");
        let dir = TempDir::new().expect("PKI directory");
        fs::write(dir.path().join("ca.pem"), ca_certificate.pem()).expect("test CA file");
        fs::write(dir.path().join("other-ca.pem"), other_certificate.pem())
            .expect("unrelated CA file");
        TestPki {
            leaf: identity(&["127.0.0.1"], Some(&ca)),
            wrong_name: identity(&["wrong.example"], Some(&ca)),
            self_signed: identity(&["127.0.0.1"], None),
            dir,
        }
    }

    /// The test CA's PEM file.
    pub fn ca_file(&self) -> PathBuf {
        self.dir.path().join("ca.pem")
    }

    /// A CA that signed nothing the fronts show.
    pub fn other_ca_file(&self) -> PathBuf {
        self.dir.path().join("other-ca.pem")
    }

    /// A directory beside the CA files, for a test that needs files of its own there.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }
}

/// How long a TLS front waits for a handshake, and `connections` for each to finish.
const FRONT_TIMEOUT: Duration = Duration::from_secs(30);

/// What became of one connection a TLS front accepted.
#[derive(Default)]
struct FrontConnection {
    /// `None` until the handshake ends; then its outcome, with the alert the client sent if it
    /// sent one.
    handshake: Mutex<Option<Result<(), Option<rustls::AlertDescription>>>>,
    /// The client sent application data after a completed handshake.
    application_bytes: AtomicBool,
    done: AtomicBool,
}

/// One connection a TLS front accepted, once it finished.
#[derive(Debug, Clone)]
pub struct FrontRecord {
    /// `Ok` for a completed handshake; else the alert the client sent, if any. `Err(None)`
    /// also stands for a handshake that never ended.
    pub handshake: Result<(), Option<rustls::AlertDescription>>,
    /// Bytes followed the handshake, so the client went on to send a request.
    pub application_bytes: bool,
}

impl FrontRecord {
    /// The handshake failed with the client refusing the certificate it was shown.
    pub fn failed_with_certificate_alert(&self) -> bool {
        use rustls::AlertDescription as A;
        matches!(
            self.handshake,
            Err(Some(
                A::BadCertificate
                    | A::UnsupportedCertificate
                    | A::CertificateRevoked
                    | A::CertificateExpired
                    | A::CertificateUnknown
                    | A::UnknownCA
            ))
        )
    }
}

/// A TLS terminator on loopback that relays each connection's plaintext to the counting proxy.
///
/// It runs on its `TestS3`'s runtime and records every connection it accepts: how the
/// handshake ended and whether any application data followed. A connection reaches the proxy
/// only once the client has sent application data, so a client that refused the certificate
/// leaves the proxy untouched and every proxy assertion keeps its meaning.
///
/// Tests never count handshakes as the verdict. httplib completes the handshake and only then
/// checks the certificate, so a refusing reader shows a completed handshake closed without a
/// request; the writer's TLS stack refuses inside the handshake, and its retries can repeat it.
pub struct TlsFront {
    addr: SocketAddr,
    connections: Arc<Mutex<Vec<Arc<FrontConnection>>>>,
    acceptor: tokio::task::JoinHandle<()>,
}

impl TestS3 {
    /// A TLS front before this endpoint's proxy, showing `identity`.
    pub fn tls_front(&self, identity: &FrontIdentity) -> TlsFront {
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("TLS versions")
            .with_no_client_auth()
            .with_single_cert(
                vec![identity.certificate.clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key.clone())),
            )
            .expect("TLS front configuration");
        let tls = tokio_rustls::TlsAcceptor::from(Arc::new(config));

        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("cannot bind the TLS front: {e}"));
        listener
            .set_nonblocking(true)
            .expect("nonblocking TLS front listener");
        let addr = listener.local_addr().expect("TLS front address");
        let upstream = self.proxy.addr;
        let connections: Arc<Mutex<Vec<Arc<FrontConnection>>>> = Arc::default();
        let accepted = Arc::clone(&connections);
        let runtime = self
            .runtime
            .as_ref()
            .expect("the endpoint runtime is running");
        let acceptor = runtime.spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("TLS front listener");
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    continue;
                };
                // Counted at accept, so a test that asks once its client exited sees every
                // connection the client made.
                let connection = Arc::new(FrontConnection::default());
                accepted.lock().unwrap().push(Arc::clone(&connection));
                let tls = tls.clone();
                tokio::spawn(async move {
                    let handshake = tokio::time::timeout(FRONT_TIMEOUT, tls.accept(socket)).await;
                    let mut stream = match handshake {
                        Ok(Ok(stream)) => {
                            *connection.handshake.lock().unwrap() = Some(Ok(()));
                            stream
                        }
                        Ok(Err(error)) => {
                            let alert = error
                                .get_ref()
                                .and_then(|e| e.downcast_ref::<rustls::Error>())
                                .and_then(|e| match e {
                                    rustls::Error::AlertReceived(alert) => Some(*alert),
                                    _ => None,
                                });
                            *connection.handshake.lock().unwrap() = Some(Err(alert));
                            connection.done.store(true, Ordering::SeqCst);
                            return;
                        }
                        Err(_) => {
                            *connection.handshake.lock().unwrap() = Some(Err(None));
                            connection.done.store(true, Ordering::SeqCst);
                            return;
                        }
                    };
                    let mut first = vec![0u8; 16 * 1024];
                    let n = match tokio::time::timeout(FRONT_TIMEOUT, stream.read(&mut first)).await
                    {
                        Ok(Ok(n)) => n,
                        _ => 0,
                    };
                    if n > 0 {
                        connection.application_bytes.store(true, Ordering::SeqCst);
                        if let Ok(mut proxy) = tokio::net::TcpStream::connect(upstream).await
                            && proxy.write_all(&first[..n]).await.is_ok()
                        {
                            let _ = tokio::io::copy_bidirectional(&mut stream, &mut proxy).await;
                        }
                    }
                    let _ = stream.shutdown().await;
                    connection.done.store(true, Ordering::SeqCst);
                });
            }
        });
        TlsFront {
            addr,
            connections,
            acceptor,
        }
    }
}

impl TlsFront {
    /// The endpoint URL a client is given.
    pub fn url(&self) -> String {
        format!("https://{}", self.addr)
    }

    /// Every connection accepted so far, once each has finished, in accept order.
    pub fn connections(&self) -> Vec<FrontRecord> {
        let deadline = Instant::now() + FRONT_TIMEOUT + Duration::from_secs(5);
        let connections = self.connections.lock().unwrap().clone();
        connections
            .iter()
            .map(|connection| {
                while !connection.done.load(Ordering::SeqCst) {
                    assert!(
                        Instant::now() < deadline,
                        "a TLS front connection is still open after {FRONT_TIMEOUT:?}"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                FrontRecord {
                    handshake: connection.handshake.lock().unwrap().unwrap_or(Err(None)),
                    application_bytes: connection.application_bytes.load(Ordering::SeqCst),
                }
            })
            .collect()
    }
}

impl Drop for TlsFront {
    fn drop(&mut self) {
        self.acceptor.abort();
    }
}

/// One request as the proxy relayed it.
#[derive(Debug, Clone)]
pub struct ProxyRecord {
    pub method: String,
    /// The request target as sent: a path, percent-encoded, with any query.
    pub target: String,
    pub range: Option<String>,
    pub status: u16,
    pub content_range: Option<String>,
    /// Everything sent back to the client: status line, headers and body.
    pub bytes_to_client: u64,
    /// The names of the request's `x-amz-*` headers, lowercased.
    pub amz_headers: Vec<String>,
}

impl ProxyRecord {
    /// The object key the target names, percent-decoded, without the bucket or a query.
    pub fn key(&self) -> String {
        let path = self.target.split('?').next().unwrap_or("");
        let path = path.strip_prefix('/').unwrap_or(path);
        let key = path
            .strip_prefix(BUCKET)
            .map(|rest| rest.strip_prefix('/').unwrap_or(rest))
            .unwrap_or(path);
        percent_decode(key)
    }

    /// `(first, last)` of a `bytes=first-last` range; `None` for anything else.
    pub fn bounded_range(&self) -> Option<(u64, u64)> {
        let spec = self.range.as_deref()?.trim().strip_prefix("bytes=")?;
        let (first, last) = spec.split_once('-')?;
        Some((first.trim().parse().ok()?, last.trim().parse().ok()?))
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if bytes[i] == b'%'
            && let Some(byte) = hex
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("percent-decoded key is UTF-8")
}

#[derive(Default)]
struct ProxyState {
    records: Mutex<Vec<ProxyRecord>>,
    errors: Mutex<Vec<String>>,
    /// Every connection accepted, in accept order.
    connections: Mutex<Vec<Arc<ProxyConnection>>>,
}

/// What became of one accepted connection.
#[derive(Default)]
struct ProxyConnection {
    /// The client sent at least one byte.
    carried: AtomicBool,
    /// The request those bytes made was recorded.
    recorded: AtomicBool,
    /// The relay is finished with the connection, whatever the outcome.
    done: AtomicBool,
}

/// How long `assert_clean` waits for connections still relaying a request.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);

/// A std-only HTTP/1.1 relay that records every request it forwards.
///
/// A connection carries at most one request: the proxy forwards it with `Connection: close`,
/// reads the response to EOF, records the request, relays the response with
/// `Connection: close`, and closes. A client that pipelined a second request on the same
/// connection is recorded as an error rather than silently missed. A connection that closes or
/// idles out before sending a byte carries no request and is not one: a client may open a
/// pooled or speculative connection and never use it, or drop one when a retry races a
/// refusal. `Host` passes through unchanged, since SigV4 signs it.
struct CountingProxy {
    addr: SocketAddr,
    state: Arc<ProxyState>,
    stopping: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

const HEAD_LIMIT: usize = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(60);

impl CountingProxy {
    fn start(upstream: SocketAddr) -> CountingProxy {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("cannot bind the counting proxy: {e}"));
        let addr = listener.local_addr().expect("proxy address");
        let state = Arc::new(ProxyState::default());
        let stopping = Arc::new(AtomicBool::new(false));
        let acceptor = {
            let state = Arc::clone(&state);
            let stopping = Arc::clone(&stopping);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let connection = Arc::new(ProxyConnection::default());
                    state
                        .connections
                        .lock()
                        .unwrap()
                        .push(Arc::clone(&connection));
                    let state = Arc::clone(&state);
                    thread::spawn(move || {
                        if let Err(e) = relay(stream, upstream, &state, &connection) {
                            state.errors.lock().unwrap().push(e.to_string());
                        }
                        connection.done.store(true, Ordering::SeqCst);
                    });
                }
            })
        };
        CountingProxy {
            addr,
            state,
            stopping,
            acceptor: Some(acceptor),
        }
    }

    fn records(&self) -> Vec<ProxyRecord> {
        self.state.records.lock().unwrap().clone()
    }

    /// The proxy hit no error; every connection that carried a byte carried a request that was
    /// recorded; and the endpoint accepted exactly one connection per recorded request, so no
    /// request reached it except through the recorder. Connections that carried nothing are
    /// not requests and are not counted.
    ///
    /// Callers assert once their clients have exited, but a relay may still be finishing a
    /// request a client gave up on, so connections that carried bytes are waited for first.
    fn assert_clean(&self, endpoint_connections: &AtomicUsize) {
        let deadline = Instant::now() + SETTLE_TIMEOUT;
        loop {
            let relaying = self
                .connections()
                .iter()
                .filter(|c| c.carried.load(Ordering::SeqCst) && !c.done.load(Ordering::SeqCst))
                .count();
            if relaying == 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{relaying} connection(s) still relaying a request after {SETTLE_TIMEOUT:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let errors = self.state.errors.lock().unwrap().clone();
        assert!(errors.is_empty(), "the counting proxy failed: {errors:#?}");
        let connections = self.connections();
        let carried = connections
            .iter()
            .filter(|c| c.carried.load(Ordering::SeqCst))
            .count();
        let unrecorded = connections
            .iter()
            .filter(|c| c.carried.load(Ordering::SeqCst) && !c.recorded.load(Ordering::SeqCst))
            .count();
        assert_eq!(
            unrecorded, 0,
            "{unrecorded} of {carried} connection(s) that carried bytes have no recorded request"
        );
        let requests = self.state.records.lock().unwrap().len();
        let reached = endpoint_connections.load(Ordering::SeqCst);
        assert_eq!(
            reached, requests,
            "the endpoint accepted {reached} connection(s) for {requests} recorded request(s)"
        );
    }

    fn connections(&self) -> Vec<Arc<ProxyConnection>> {
        self.state.connections.lock().unwrap().clone()
    }

    fn stop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // The acceptor is blocked in accept(); one connection wakes it to see the flag.
        let _wake = TcpStream::connect(self.addr);
        if let Some(acceptor) = self.acceptor.take() {
            acceptor.join().expect("proxy acceptor thread");
        }
    }
}

struct Head {
    first_line: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The head with `Connection: close` in place of any connection-management header.
    fn closing(&self) -> Vec<u8> {
        let mut out = format!("{}\r\n", self.first_line);
        for (name, value) in &self.headers {
            let lower = name.to_ascii_lowercase();
            if lower == "connection" || lower == "keep-alive" || lower == "proxy-connection" {
                continue;
            }
            out.push_str(&format!("{name}: {value}\r\n"));
        }
        out.push_str("Connection: close\r\n\r\n");
        out.into_bytes()
    }
}

/// Read up to the blank line ending a head; return it and whatever followed.
fn read_head(stream: &mut TcpStream) -> io::Result<Option<(Head, Vec<u8>)>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > HEAD_LIMIT {
            return Err(io::Error::other("head exceeds 64 KiB"));
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::other("connection closed inside a head"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let text = String::from_utf8_lossy(&buf[..end]).to_string();
    let mut lines = text.split("\r\n");
    let first_line = lines.next().unwrap_or("").to_string();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let rest = buf[end + 4..].to_vec();
    Ok(Some((
        Head {
            first_line,
            headers,
        },
        rest,
    )))
}

fn relay(
    mut client: TcpStream,
    upstream: SocketAddr,
    state: &ProxyState,
    connection: &ProxyConnection,
) -> io::Result<()> {
    client.set_read_timeout(Some(IO_TIMEOUT))?;
    client.set_write_timeout(Some(IO_TIMEOUT))?;
    // A connection that closes, resets or idles out before sending a byte carries no request:
    // a client's unused pooled or speculative connection, one dropped as a retry raced a
    // refusal, or the acceptor's own wake-up at shutdown.
    match client.peek(&mut [0u8; 1]) {
        Ok(n) if n > 0 => connection.carried.store(true, Ordering::SeqCst),
        _ => return Ok(()),
    }
    let Some((request, mut body)) = read_head(&mut client)? else {
        return Err(io::Error::other(
            "connection closed before a head it had begun",
        ));
    };
    if request
        .header("transfer-encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    {
        return Err(io::Error::other(
            "chunked request bodies are not relayed by the counting proxy",
        ));
    }
    let length: usize = request
        .header("content-length")
        .map(|v| {
            v.parse()
                .map_err(|_| io::Error::other("bad Content-Length"))
        })
        .transpose()?
        .unwrap_or(0);
    while body.len() < length {
        let mut chunk = vec![0u8; length - body.len()];
        let n = client.read(&mut chunk)?;
        if n == 0 {
            return Err(io::Error::other("connection closed inside a body"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    if body.len() > length {
        return Err(io::Error::other(format!(
            "{} byte(s) pipelined after a request; one request per connection is the rule",
            body.len() - length
        )));
    }

    let mut parts = request.first_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();

    let mut server = TcpStream::connect(upstream)?;
    server.set_read_timeout(Some(IO_TIMEOUT))?;
    server.write_all(&request.closing())?;
    server.write_all(&body)?;

    let Some((response, mut response_body)) = read_head(&mut server)? else {
        return Err(io::Error::other(format!(
            "the endpoint closed without answering {method} {target}"
        )));
    };
    server.read_to_end(&mut response_body)?;
    let status = response
        .first_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Recorded before the client sees a byte of the response, so a client that has its answer
    // never outruns the record. The relay back is best effort: the request reached the
    // endpoint and is accounted for, whether or not the client stayed to read the answer.
    let head = response.closing();
    let bytes_to_client = (head.len() + response_body.len()) as u64;
    state.records.lock().unwrap().push(ProxyRecord {
        method,
        target,
        range: request.header("range").map(str::to_string),
        status,
        content_range: response.header("content-range").map(str::to_string),
        bytes_to_client,
        amz_headers: request
            .headers
            .iter()
            .map(|(name, _)| name.to_ascii_lowercase())
            .filter(|name| name.starts_with("x-amz-"))
            .collect(),
    });
    connection.recorded.store(true, Ordering::SeqCst);
    let _ = client
        .write_all(&head)
        .and_then(|()| client.write_all(&response_body))
        .and_then(|()| client.flush());
    let _ = client.shutdown(Shutdown::Both);
    Ok(())
}

/// Empty `HOME`, `TMPDIR` and working directory for one child, plus fixture credential and
/// config file paths, checked empty afterwards.
pub struct Hermetic {
    home: TempDir,
    tmp: TempDir,
    cwd: TempDir,
    aws: TempDir,
}

impl Hermetic {
    pub fn new() -> Hermetic {
        Hermetic {
            home: TempDir::new().unwrap(),
            tmp: TempDir::new().unwrap(),
            cwd: TempDir::new().unwrap(),
            aws: TempDir::new().unwrap(),
        }
    }

    pub fn credentials_file(&self) -> PathBuf {
        self.aws.path().join("credentials")
    }

    pub fn config_file(&self) -> PathBuf {
        self.aws.path().join("config")
    }

    pub fn home(&self) -> &Path {
        self.home.path()
    }

    /// A command for a project binary.
    pub fn command(&self, binary: &str) -> Command {
        self.command_for(&binary_path(binary))
    }

    /// A command for any program, with every variable that could steer an S3 client removed:
    /// `AWS_*`, `DUCKDB_*` and every `*_PROXY`, in any case, OpenSSL's `SSL_CERT_FILE` and
    /// `SSL_CERT_DIR`, and the tools' own `XDU_INDEX` and `XDU_EXTENSION_DIR`. The test adds
    /// back exactly what it means to.
    pub fn command_for(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        for (name, _) in std::env::vars_os() {
            let upper = name.to_string_lossy().to_ascii_uppercase();
            if upper.starts_with("AWS_")
                || upper.starts_with("DUCKDB_")
                || upper.ends_with("_PROXY")
                || upper == "SSL_CERT_FILE"
                || upper == "SSL_CERT_DIR"
                || upper == "XDU_INDEX"
                || upper == "XDU_EXTENSION_DIR"
            {
                cmd.env_remove(&name);
            }
        }
        cmd.env("HOME", self.home.path())
            .env("TMPDIR", self.tmp.path())
            .env("AWS_SHARED_CREDENTIALS_FILE", self.credentials_file())
            .env("AWS_CONFIG_FILE", self.config_file())
            .current_dir(self.cwd.path());
        cmd
    }

    /// The child wrote nothing under `HOME`, `TMPDIR` or its working directory.
    pub fn assert_left_empty(&self) {
        for (what, dir) in [
            ("HOME", self.home.path()),
            ("TMPDIR", self.tmp.path()),
            ("the working directory", self.cwd.path()),
        ] {
            let left: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            assert!(left.is_empty(), "{what} was written: {left:#?}");
        }
    }
}

/// Accept on `listener` until `stopping` is set, handing each connection to `serve` on its own
/// thread. `stop_acceptor` wakes it with one connection of its own, which is never served.
fn spawn_acceptor(
    listener: TcpListener,
    stopping: Arc<AtomicBool>,
    serve: impl Fn(TcpStream) + Clone + Send + 'static,
) -> JoinHandle<()> {
    thread::spawn(move || {
        for stream in listener.incoming() {
            if stopping.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let serve = serve.clone();
            thread::spawn(move || serve(stream));
        }
    })
}

fn stop_acceptor(addr: SocketAddr, stopping: &AtomicBool, acceptor: &mut Option<JoinHandle<()>>) {
    stopping.store(true, Ordering::SeqCst);
    let _wake = TcpStream::connect(addr);
    if let Some(acceptor) = acceptor.take() {
        acceptor.join().expect("acceptor thread");
    }
}

/// A loopback listener that stands for a host no request should reach: it accepts every
/// connection, keeps the first bytes the client sent, and closes it. It speaks no protocol, so
/// a TLS client fails its handshake here, but the connection is counted first.
///
/// Every connection counts, an empty one included. Nothing but the code under test is given
/// this address, so dialling it at all is the defect a test looks for; unlike the counting
/// proxy, where a client's unused connection is harmless, here there is no harmless one.
pub struct CountingListener {
    addr: SocketAddr,
    openings: Arc<Mutex<Vec<Arc<Opening>>>>,
    stopping: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

/// One accepted connection: its first bytes, once read.
#[derive(Default)]
struct Opening {
    bytes: Mutex<Option<Vec<u8>>>,
}

/// How long a `CountingListener` waits for a client's first bytes.
const OPENING_TIMEOUT: Duration = Duration::from_secs(5);

impl CountingListener {
    pub fn start() -> CountingListener {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("cannot bind the counting listener: {e}"));
        let addr = listener.local_addr().expect("listener address");
        let openings = Arc::new(Mutex::new(Vec::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let acceptor = {
            let openings = Arc::clone(&openings);
            let stopping = Arc::clone(&stopping);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(mut stream) = stream else { continue };
                    // Counted here, at accept, not once its bytes are in: a test that asks
                    // after its client exited sees every connection the client made.
                    let opening = Arc::new(Opening::default());
                    openings.lock().unwrap().push(Arc::clone(&opening));
                    thread::spawn(move || {
                        let _ = stream.set_read_timeout(Some(OPENING_TIMEOUT));
                        let mut first = [0u8; 16];
                        let n = stream.read(&mut first).unwrap_or(0);
                        *opening.bytes.lock().unwrap() = Some(first[..n].to_vec());
                    });
                }
            })
        };
        CountingListener {
            addr,
            openings,
            stopping,
            acceptor: Some(acceptor),
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The first bytes of every connection accepted so far, one entry per connection, empty
    /// for a connection that sent nothing. Waits for each connection's first read to finish.
    pub fn openings(&self) -> Vec<Vec<u8>> {
        let openings = self.openings.lock().unwrap().clone();
        let deadline = Instant::now() + OPENING_TIMEOUT + Duration::from_secs(1);
        openings
            .iter()
            .map(|opening| {
                loop {
                    if let Some(bytes) = opening.bytes.lock().unwrap().clone() {
                        break bytes;
                    }
                    if Instant::now() >= deadline {
                        break Vec::new();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            })
            .collect()
    }
}

impl Drop for CountingListener {
    fn drop(&mut self) {
        stop_acceptor(self.addr, &self.stopping, &mut self.acceptor);
    }
}

/// An HTTP `CONNECT` proxy on loopback that records the target of every tunnel asked of it and
/// opens a tunnel only to `tunnel_to`; any other target is refused with a 403. A client pointed
/// at it may name any host, remote ones included, and nothing leaves loopback, since the
/// client hands the host name to the proxy instead of resolving it.
pub struct ConnectRecorder {
    addr: SocketAddr,
    targets: Arc<Mutex<Vec<String>>>,
    stopping: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

impl ConnectRecorder {
    pub fn start(tunnel_to: SocketAddr) -> ConnectRecorder {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("cannot bind the CONNECT recorder: {e}"));
        let addr = listener.local_addr().expect("recorder address");
        let targets = Arc::new(Mutex::new(Vec::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let kept = Arc::clone(&targets);
        let acceptor = spawn_acceptor(listener, Arc::clone(&stopping), move |mut client| {
            let _ = client.set_read_timeout(Some(Duration::from_secs(10)));
            let Ok(Some((head, _))) = read_head(&mut client) else {
                return;
            };
            let mut parts = head.first_line.split_whitespace();
            let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            kept.lock().unwrap().push(format!("{method} {target}"));
            if !method.eq_ignore_ascii_case("CONNECT") || target != tunnel_to.to_string() {
                let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
                return;
            }
            let Ok(mut upstream) = TcpStream::connect(tunnel_to) else {
                return;
            };
            let _ = client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
            let _ = upstream.set_read_timeout(Some(Duration::from_secs(10)));
            let (Ok(mut client_back), Ok(mut upstream_back)) =
                (client.try_clone(), upstream.try_clone())
            else {
                return;
            };
            let back = thread::spawn(move || {
                let _ = io::copy(&mut upstream_back, &mut client_back);
                let _ = client_back.shutdown(Shutdown::Both);
            });
            let _ = io::copy(&mut client, &mut upstream);
            let _ = upstream.shutdown(Shutdown::Both);
            let _ = back.join();
        });
        ConnectRecorder {
            addr,
            targets,
            stopping,
            acceptor: Some(acceptor),
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `METHOD target` of every request asked of the proxy so far.
    pub fn requests(&self) -> Vec<String> {
        self.targets.lock().unwrap().clone()
    }
}

impl Drop for ConnectRecorder {
    fn drop(&mut self) {
        stop_acceptor(self.addr, &self.stopping, &mut self.acceptor);
    }
}

/// A loopback endpoint that refuses every request with `status` and an AWS-shaped error body
/// echoing the request's signed headers, as S3's `SignatureDoesNotMatch` body echoes its
/// canonical request and an `InvalidToken` body echoes the token. A client that prints the
/// body it was sent prints whatever it signed, a session token included.
pub struct EchoingRefusal {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
    stopping: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

impl EchoingRefusal {
    pub fn start(status: u16, code: &'static str) -> EchoingRefusal {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("cannot bind the echoing endpoint: {e}"));
        let addr = listener.local_addr().expect("echoing endpoint address");
        let requests = Arc::new(AtomicUsize::new(0));
        let stopping = Arc::new(AtomicBool::new(false));
        let counted = Arc::clone(&requests);
        let acceptor = spawn_acceptor(listener, Arc::clone(&stopping), move |mut client| {
            let _ = client.set_read_timeout(Some(IO_TIMEOUT));
            let Ok(Some((head, mut body))) = read_head(&mut client) else {
                return;
            };
            counted.fetch_add(1, Ordering::SeqCst);
            let length: usize = head
                .header("content-length")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let mut chunk = [0u8; 8192];
            while body.len() < length {
                match client.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => body.extend_from_slice(&chunk[..n]),
                }
            }
            let mut canonical = format!("{}\n", head.first_line);
            for (name, value) in &head.headers {
                canonical.push_str(&format!("{}:{}\n", name.to_ascii_lowercase(), value));
            }
            let token = head.header("x-amz-security-token").unwrap_or("");
            let xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{code}</Code>\
                 <Message>The request signature we calculated does not match the signature \
                 you provided.</Message><Token-0>{token}</Token-0>\
                 <CanonicalRequest>{canonical}</CanonicalRequest></Error>"
            );
            let reason = if status == 403 {
                "Forbidden"
            } else {
                "Bad Request"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/xml\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{xml}",
                xml.len()
            );
            let _ = client.write_all(response.as_bytes());
            let _ = client.shutdown(Shutdown::Both);
        });
        EchoingRefusal {
            addr,
            requests,
            stopping,
            acceptor: Some(acceptor),
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for EchoingRefusal {
    fn drop(&mut self) {
        stop_acceptor(self.addr, &self.stopping, &mut self.acceptor);
    }
}

/// Run a command; return (stdout, stderr, success).
pub fn output_of(cmd: &mut Command) -> (String, String, bool) {
    let output = cmd.output().expect("spawn child");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Neither secret nor the session token appears in `text`.
pub fn assert_no_secret(what: &str, text: &str) {
    for secret in [
        SECRET_KEY,
        WRONG_SECRET_KEY,
        SESSION_TOKEN,
        READ_ONLY_SECRET_KEY,
    ] {
        assert!(!text.contains(secret), "{what} carries a secret: {text}");
    }
}

/// A source tree whose index has one chunk large enough that a footer read cannot cover it:
/// `files` files under `<root>/<partition>/`, each named by 200 pseudo-random hex characters,
/// so the path column does not compress away.
pub fn wide_source_tree(root: &Path, partition: &str, files: usize) {
    let dir = root.join(partition);
    fs::create_dir_all(&dir).unwrap();
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..files {
        fs::write(dir.join(hex_noise(&mut state, 200)), b"x").unwrap();
    }
}

/// `count` lowercase hex characters from a splitmix64 stream: cheap, deterministic, and far
/// from compressible, so a column of them keeps its size on disk.
pub fn hex_noise(state: &mut u64, count: usize) -> String {
    let mut text = String::with_capacity(count + 16);
    while text.len() < count {
        *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        text.push_str(&format!("{z:016x}"));
    }
    text.truncate(count);
    text
}

/// Set a file's access and modification times, in seconds since the epoch.
pub fn set_times(file: &Path, atime: i64, mtime: i64) {
    let c_path = std::ffi::CString::new(file.to_str().unwrap()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: atime as libc::time_t,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: mtime as libc::time_t,
            tv_nsec: 0,
        },
    ];
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(
        rc,
        0,
        "utimensat {}: {}",
        file.display(),
        io::Error::last_os_error()
    );
}

/// A source tree for comparing a local and an S3 query row for row.
///
/// Three partitions of different sizes (`alice` with nested directories, `bob`, and loose
/// top-level files for `__root__`), so `--top` has no ties. Every file has its own size, atime
/// and mtime, so each sort has one answer; the times sit half a step between whole days, so a
/// day threshold computed seconds later by a second query cannot move a file across it. Names
/// are long hex strings, so every chunk is far larger than the footer read DuckDB opens it
/// with, and a third of them end in `.log`, the rest in `.txt`.
pub fn varied_source_tree(root: &Path) {
    const STEP: i64 = 8640;
    let groups: [(&str, usize); 4] = [
        ("alice/a", 400),
        ("alice/b/c", 300),
        ("bob", 450),
        ("", 250),
    ];
    let total: usize = groups.iter().map(|(_, n)| n).sum();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut state: u64 = 0x5eed_0000_0000_0001;
    let mut i = 0usize;
    for (dir, files) in groups {
        let dir = root.join(dir);
        fs::create_dir_all(&dir).unwrap();
        for _ in 0..files {
            let ext = if i.is_multiple_of(3) { "log" } else { "txt" };
            let file = dir.join(format!("{}.{ext}", hex_noise(&mut state, 180)));
            fs::write(&file, vec![b'x'; i + 1]).unwrap();
            let j = (i * 7) % total;
            set_times(
                &file,
                now - (i as i64 * STEP + STEP / 2),
                now - (j as i64 * STEP + STEP / 2),
            );
            i += 1;
        }
    }
}

/// Write an index of `partitions` x `chunks` chunk files of `rows` rows each under `index`,
/// encoded by xdu's own chunk encoder, plus a completion marker of the current format.
///
/// Paths are 200 hex characters of noise, so the path column dominates every chunk and does
/// not compress away. Returns the total row count.
pub fn encoded_index(index: &Path, partitions: &[&str], chunks: usize, rows: usize) -> u64 {
    use arrow::array::{Int64Array, StringArray};
    use arrow::record_batch::RecordBatch;
    use std::sync::Arc;

    let mut state: u64 = 0x0b5e_55ed_0000_0002;
    let mut total = 0u64;
    for partition in partitions {
        let dir = index.join(partition);
        fs::create_dir_all(&dir).unwrap();
        for chunk in 0..chunks {
            let paths: Vec<String> = (0..rows)
                .map(|_| format!("/src/{partition}/{}", hex_noise(&mut state, 200)))
                .collect();
            let ints = |base: i64| -> Arc<Int64Array> {
                Arc::new(Int64Array::from_iter_values(
                    (0..rows as i64).map(|r| base + r),
                ))
            };
            let batch = RecordBatch::try_new(
                xdu::get_schema(),
                vec![
                    Arc::new(StringArray::from(paths)),
                    ints(1),
                    Arc::new(Int64Array::from_value(1000, rows)),
                    Arc::new(Int64Array::from_value(1000, rows)),
                    Arc::new(Int64Array::from_value(0o644, rows)),
                    ints(1_700_000_000),
                    ints(1_690_000_000),
                    ints(1_680_000_000),
                ],
            )
            .unwrap();
            let file = fs::File::create(dir.join(xdu::crawl::chunk_final_name(chunk))).unwrap();
            xdu::crawl::encode_chunk(file, &batch).unwrap();
            total += rows as u64;
        }
    }
    let stats = xdu::crawl::CrawlStats {
        files: total,
        ..Default::default()
    };
    fs::write(
        index.join(xdu::COMPLETION_MARKER),
        xdu::crawl::completion_marker_contents(&stats, 1_700_000_000),
    )
    .unwrap();
    total
}

/// A completion marker body of the current format, padded with an unread key to exactly
/// `len` bytes.
pub fn padded_marker(len: usize) -> Vec<u8> {
    let mut body =
        xdu::crawl::completion_marker_contents(&xdu::crawl::CrawlStats::default(), 1_700_000_000);
    let pad = len
        .checked_sub(body.len() + "pad=\n".len())
        .expect("the marker is longer than the requested length");
    body.push_str(&format!("pad={}\n", "x".repeat(pad)));
    assert_eq!(body.len(), len);
    body.into_bytes()
}
