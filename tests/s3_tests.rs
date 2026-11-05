//! S3 index tests, in two tiers.
//!
//! Tests without `#[ignore]` need nothing but the binaries and run in every `cargo test`. Wire
//! tests talk to a loopback S3 endpoint through the pinned `httpfs`, so they are
//! `#[ignore]`d: a machine without the extension lists them as ignored rather than passing them.
//! CI fetches the extension and runs this file with `--include-ignored` through
//! `spec/s3-index-target/verify/wire.sh`, which fails unless every listed test passed.
//!
//! Wire tests are named by family (`harness_`, then the reader, writer and viewer families),
//! so a phase's gate selects its own. Each goes through the counting proxy and asserts that no
//! GET of a chunk covered the whole object. None returns early: `TestS3::start()` panics when
//! it cannot run, so an opted-in run cannot report a test that did not happen as passed.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use duckdb::Connection;
use tempfile::TempDir;

use common::build_index;
use common::create_test_file;
use common::s3::{
    ACCESS_KEY, BUCKET, EXTENSION_DIR_VAR, Hermetic, ProxyRecord, READ_ONLY_ACCESS_KEY,
    READ_ONLY_SECRET_KEY, REFUSED_PARTITION, SECRET_KEY, SESSION_TOKEN, TestS3, WRONG_SECRET_KEY,
    assert_no_secret, encoded_index, output_of, padded_marker, resolve_settings,
    test_extension_dir, varied_source_tree, wide_source_tree,
};
use xdu::engine::open_s3_connection_from;
use xdu::{COMPLETION_MARKER, MARKER_READ_LIMIT};

/// Where the seeded index lives in the bucket.
const PREFIX: &str = "idx";
/// Rows in the seeded index: enough 200-character paths that its one chunk is far larger than
/// the footer read DuckDB opens a Parquet file with.
const ROWS: usize = 1500;

/// A loopback endpoint holding a real index, built by `xdu` from a wide source tree.
fn seeded() -> (TestS3, TempDir) {
    let s3 = TestS3::start();
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    let index = work.path().join("index");
    wide_source_tree(&source, "alice", ROWS);
    build_index(&source, &index);
    s3.seed_index(&index, PREFIX);
    (s3, work)
}

fn count_rows(conn: &Connection, s3: &TestS3) -> duckdb::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM read_parquet(?)",
        [s3.url(&format!("{PREFIX}/*/*.parquet"))],
        |row| row.get(0),
    )
}

/// The chunk keys fetched with a bounded range.
fn range_fetched_keys(s3: &TestS3) -> BTreeSet<String> {
    s3.requests()
        .into_iter()
        .filter(|r| r.method == "GET" && r.bounded_range().is_some())
        .map(|r| r.key())
        .filter(|k| k.ends_with(".parquet"))
        .collect()
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn harness_connection_reads_a_seeded_index_through_the_proxy() {
    let (s3, _work) = seeded();
    let chunks = s3.parquet_sizes(PREFIX);
    assert!(!chunks.is_empty(), "the seeded index holds no chunk");

    let conn = open_s3_connection_from(&s3.settings(), s3.extension_dir())
        .unwrap_or_else(|e| panic!("open_s3_connection failed: {e:#}"));
    assert_eq!(count_rows(&conn, &s3).unwrap(), ROWS as i64);

    // The meter saw the read: every chunk was fetched by range, through the proxy.
    let chunk_keys: BTreeSet<String> = chunks.keys().cloned().collect();
    assert_eq!(range_fetched_keys(&s3), chunk_keys);
    s3.assert_no_whole_parquet_get();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn harness_wrong_secret_is_refused_without_echoing_it() {
    let (s3, _work) = seeded();
    let settings = resolve_settings(&s3.proxy_url(), WRONG_SECRET_KEY, None);

    // Creating the secret contacts nothing; the refusal comes from the endpoint on first use.
    let conn = open_s3_connection_from(&settings, s3.extension_dir())
        .unwrap_or_else(|e| panic!("open_s3_connection failed: {e:#}"));
    let err = count_rows(&conn, &s3).unwrap_err().to_string();
    assert!(err.contains("403"), "not a 403-class refusal: {err}");
    assert_no_secret("the refusal", &err);

    let requests = s3.requests();
    assert!(
        requests.iter().any(|r| r.status == 403),
        "the endpoint never refused a request: {requests:#?}"
    );
    assert!(
        requests.iter().all(|r| r.status >= 400),
        "the endpoint served a request signed with the wrong secret: {requests:#?}"
    );
    s3.assert_no_whole_parquet_get();
    s3.assert_proxy_clean();
}

/// Set in the re-executed child to the proxy URL it should read through.
const STRAY_CHILD_VAR: &str = "XDU_TEST_STRAY_ENDPOINT_PROXY";
const STRAY_ENDPOINT: &str = "127.0.0.1:1";
const STRAY_KEY_ID: &str = "stray-key-id";

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn harness_stray_duckdb_s3_environment_does_not_redirect_the_connection() {
    // httpfs reads DUCKDB_S3_ENDPOINT and the AWS key variables from the process environment
    // when it loads. The test process's environment is shared by every test thread, so the
    // stray values go to a re-executed copy of this test instead.
    match std::env::var(STRAY_CHILD_VAR) {
        Ok(proxy_url) => stray_environment_child(&proxy_url),
        Err(_) => stray_environment_parent(),
    }
}

fn stray_environment_parent() {
    let (s3, _work) = seeded();
    let hermetic = Hermetic::new();
    let exe = std::env::current_exe().unwrap();
    let mut cmd = hermetic.command_for(&exe);
    cmd.args([
        "harness_stray_duckdb_s3_environment_does_not_redirect_the_connection",
        "--exact",
        "--include-ignored",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(STRAY_CHILD_VAR, s3.proxy_url())
    .env(EXTENSION_DIR_VAR, s3.extension_dir())
    .env("DUCKDB_S3_ENDPOINT", STRAY_ENDPOINT)
    .env("AWS_ACCESS_KEY_ID", STRAY_KEY_ID)
    .env("AWS_SECRET_ACCESS_KEY", WRONG_SECRET_KEY);
    let (out, err, ok) = output_of(&mut cmd);
    assert!(ok, "the child read failed:\n{out}\n{err}");
    assert!(
        out.contains(&format!("stray-environment child counted {ROWS} rows")),
        "the child did not report its read:\n{out}"
    );
    assert!(out.contains("1 passed"), "the child ran no test:\n{out}");
    assert_no_secret("the child's stdout", &out);
    assert_no_secret("the child's stderr", &err);

    let chunk_keys: BTreeSet<String> = s3.parquet_sizes(PREFIX).keys().cloned().collect();
    assert_eq!(range_fetched_keys(&s3), chunk_keys);
    s3.assert_no_whole_parquet_get();
    s3.assert_proxy_clean();
    hermetic.assert_left_empty();
}

fn stray_environment_child(proxy_url: &str) {
    assert_eq!(
        std::env::var("DUCKDB_S3_ENDPOINT").as_deref(),
        Ok(STRAY_ENDPOINT)
    );
    let settings = resolve_settings(proxy_url, SECRET_KEY, None);
    let conn = open_s3_connection_from(&settings, &test_extension_dir())
        .unwrap_or_else(|e| panic!("open_s3_connection failed: {e:#}"));

    // httpfs took the stray values as its settings, so this read is a real contest between
    // them and the bound secret, not a read with nothing to override.
    let (endpoint, key_id): (String, String) = conn
        .query_row(
            "SELECT current_setting('s3_endpoint'), current_setting('s3_access_key_id')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        endpoint, STRAY_ENDPOINT,
        "httpfs ignored DUCKDB_S3_ENDPOINT"
    );
    assert_eq!(key_id, STRAY_KEY_ID, "httpfs ignored AWS_ACCESS_KEY_ID");

    let s3_url = format!("s3://{}/{PREFIX}/*/*.parquet", common::s3::BUCKET);
    let count: i64 = conn
        .query_row("SELECT count(*) FROM read_parquet(?)", [s3_url], |row| {
            row.get(0)
        })
        .unwrap_or_else(|e| panic!("the stray environment redirected the read: {e}"));
    println!("stray-environment child counted {count} rows");
    assert_eq!(count, ROWS as i64);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn harness_secret_listing_shows_the_secret_redacted() {
    let (s3, _work) = seeded();
    let settings = resolve_settings(&s3.proxy_url(), SECRET_KEY, Some(SESSION_TOKEN));
    let conn = open_s3_connection_from(&settings, s3.extension_dir())
        .unwrap_or_else(|e| panic!("open_s3_connection failed: {e:#}"));

    let mut stmt = conn
        .prepare("SELECT name, secret_string FROM duckdb_secrets()")
        .unwrap();
    let secrets: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(secrets.len(), 1, "expected one secret: {secrets:?}");
    let (name, listing) = &secrets[0];
    assert_eq!(name, "xdu_s3");
    assert!(
        listing.contains("secret=redacted"),
        "the secret is not shown redacted: {listing}"
    );
    assert!(
        listing.contains(&format!("key_id={ACCESS_KEY}")),
        "the listing is not this secret's: {listing}"
    );
    assert_no_secret("duckdb_secrets()", listing);

    assert_eq!(count_rows(&conn, &s3).unwrap(), ROWS as i64);
    s3.assert_no_whole_parquet_get();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn harness_settings_never_hold_the_secret() {
    let (s3, _work) = seeded();
    let settings = resolve_settings(&s3.proxy_url(), SECRET_KEY, Some(SESSION_TOKEN));
    let conn = open_s3_connection_from(&settings, s3.extension_dir())
        .unwrap_or_else(|e| panic!("open_s3_connection failed: {e:#}"));

    let mut stmt = conn
        .prepare("SELECT name, coalesce(value, '') FROM duckdb_settings()")
        .unwrap();
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    // httpfs registers its s3_* settings at load; seeing them proves the scan covered them.
    assert!(
        rows.iter().any(|(name, _)| name == "s3_secret_access_key"),
        "duckdb_settings() lists no httpfs setting: {} rows",
        rows.len()
    );
    for (name, value) in &rows {
        assert_no_secret(&format!("duckdb_settings() row {name}"), value);
    }

    assert_eq!(count_rows(&conn, &s3).unwrap(), ROWS as i64);
    s3.assert_no_whole_parquet_get();
    s3.assert_proxy_clean();
}

/// An `s3://` index with no `httpfs` where the tool looks fails naming that file and both
/// remedies, whether the locator comes from `-i` or `XDU_INDEX`, and writes nothing.
#[test]
fn s3_index_without_httpfs_names_the_path_and_both_remedies() {
    let empty_extensions = TempDir::new().unwrap();
    let expected = empty_extensions.path().join("httpfs.duckdb_extension");
    let hermetic = Hermetic::new();
    for via_env in [false, true] {
        let mut cmd = hermetic.command("xdu-find");
        cmd.env("XDU_EXTENSION_DIR", empty_extensions.path())
            .env("AWS_ACCESS_KEY_ID", ACCESS_KEY)
            .env("AWS_SECRET_ACCESS_KEY", SECRET_KEY)
            .env("AWS_ENDPOINT_URL", "http://127.0.0.1:1");
        if via_env {
            cmd.env("XDU_INDEX", "s3://xdu-test/idx").arg("--count");
        } else {
            cmd.args(["-i", "s3://xdu-test/idx", "--count"]);
        }
        let (out, err, ok) = output_of(&mut cmd);
        assert!(!ok, "xdu-find succeeded without httpfs (via_env={via_env})");
        assert!(out.is_empty(), "rows printed without httpfs: {out}");
        assert!(
            err.contains(expected.to_str().unwrap()),
            "the error does not name {}: {err}",
            expected.display()
        );
        assert!(
            err.contains("release tarball"),
            "the error does not name the release-tarball remedy: {err}"
        );
        assert!(
            err.contains("XDU_EXTENSION_DIR"),
            "the error does not name XDU_EXTENSION_DIR: {err}"
        );
        assert_no_secret("stdout", &out);
        assert_no_secret("stderr", &err);
    }
    assert_eq!(
        fs::read_dir(empty_extensions.path()).unwrap().count(),
        0,
        "the extension directory was written"
    );
    hermetic.assert_left_empty();
}

/// A local index loads no extension: with a garbage `httpfs` in `XDU_EXTENSION_DIR`, the
/// crawler and both local readers answer correctly and print nothing on stderr. A load error
/// that was swallowed would have had to print or be discarded, and `src` holds no discarded
/// `load_extension` result.
#[test]
fn local_tools_ignore_a_garbage_httpfs() {
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    let index = work.path().join("index");
    create_test_file(&source.join("user1/a.log"), 100).unwrap();
    create_test_file(&source.join("user1/b.log"), 100).unwrap();
    create_test_file(&source.join("user1/c.txt"), 100).unwrap();

    let extensions = TempDir::new().unwrap();
    let garbage = extensions.path().join("httpfs.duckdb_extension");
    fs::write(&garbage, b"not a DuckDB extension").unwrap();
    let hermetic = Hermetic::new();
    let run = |binary: &str, args: &[&str]| {
        let mut cmd = hermetic.command(binary);
        cmd.env("XDU_EXTENSION_DIR", extensions.path()).args(args);
        output_of(&mut cmd)
    };
    let index_arg = index.to_str().unwrap();

    let (_, err, ok) = run(
        "xdu",
        &["--apparent-size", "-o", index_arg, source.to_str().unwrap()],
    );
    assert!(ok, "xdu failed: {err}");
    // The crawler's stderr is its log, so it cannot be empty. It must hold INFO records only,
    // none of them about an extension: a load attempt would have logged or failed the run.
    assert!(!err.is_empty(), "xdu logged nothing");
    for line in err.lines() {
        let level = line.split_whitespace().nth(1);
        assert_eq!(level, Some("INFO"), "xdu logged more than progress: {line}");
        assert!(
            !line.contains("httpfs") && !line.contains("extension"),
            "xdu logged an extension: {line}"
        );
    }

    let (out, err, ok) = run("xdu-find", &["-i", index_arg, "--count"]);
    assert!(ok, "xdu-find failed: {err}");
    assert_eq!(out.trim(), "3");
    assert_eq!(err, "", "xdu-find wrote to stderr");

    let (out, err, ok) = run("xdu-rm", &["-i", index_arg, "-p", "*.log", "--dry-run"]);
    assert!(ok, "xdu-rm failed: {err}");
    assert!(out.contains("2 file(s) would be deleted"), "{out}");
    assert_eq!(err, "", "xdu-rm wrote to stderr");

    assert_eq!(fs::read(&garbage).unwrap(), b"not a DuckDB extension");
    assert!(
        source.join("user1/a.log").exists(),
        "a dry run deleted a file"
    );
    hermetic.assert_left_empty();
}

// =============================================================================
// xdu-find over an s3:// index
// =============================================================================

/// Run a child, asserting that neither secret nor the session token reached its output.
fn run(cmd: &mut Command) -> (String, String, bool) {
    let (out, err, ok) = output_of(cmd);
    assert_no_secret("stdout", &out);
    assert_no_secret("stderr", &err);
    (out, err, ok)
}

/// The proxy and the child's private directories after a test: every request in this bucket,
/// one per connection, no chunk fetched whole, and nothing written under `HOME`, `TMPDIR` or
/// the working directory.
fn assert_clean(s3: &TestS3, hermetic: &Hermetic) {
    s3.assert_no_whole_parquet_get();
    s3.assert_proxy_clean();
    hermetic.assert_left_empty();
}

/// A local index built by `xdu` from `varied_source_tree`, and the same files in a bucket.
struct Parity {
    s3: TestS3,
    hermetic: Hermetic,
    index: PathBuf,
    locator: String,
    _work: TempDir,
}

impl Parity {
    fn new(prefix: &str) -> Parity {
        let s3 = TestS3::start();
        let work = TempDir::new().unwrap();
        let source = work.path().join("source");
        let index = work.path().join("index");
        varied_source_tree(&source);
        build_index(&source, &index);
        s3.seed_index(&index, prefix);
        Parity {
            locator: s3.url(prefix),
            s3,
            hermetic: Hermetic::new(),
            index,
            _work: work,
        }
    }

    fn local(&self, args: &[&str]) -> (String, String, bool) {
        run(self
            .hermetic
            .command("xdu-find")
            .arg("-i")
            .arg(&self.index)
            .args(args))
    }

    fn remote(&self, args: &[&str]) -> (String, String, bool) {
        run(self
            .s3
            .find_command(&self.hermetic)
            .args(["-i", &self.locator])
            .args(args))
    }

    /// The S3 run prints exactly what the local run prints, and that is not nothing.
    fn assert_same(&self, args: &[&str]) {
        let (local, local_err, ok) = self.local(args);
        assert!(ok, "local xdu-find {args:?} failed: {local_err}");
        assert!(
            !local.trim().is_empty(),
            "local xdu-find {args:?} printed nothing"
        );
        let (remote, remote_err, ok) = self.remote(args);
        assert!(ok, "S3 xdu-find {args:?} failed: {remote_err}");
        assert!(
            local == remote,
            "S3 output differs from local for {args:?}:\n--- local\n{local}\n--- S3\n{remote}"
        );
    }

    fn finish(&self) {
        let chunks: BTreeSet<String> = self
            .s3
            .parquet_sizes(self.locator.trim_start_matches(&format!("s3://{BUCKET}/")))
            .into_keys()
            .collect();
        assert!(!chunks.is_empty(), "the bucket holds no chunk");
        let fetched = range_fetched_keys(&self.s3);
        assert!(
            fetched.is_subset(&chunks) && !fetched.is_empty(),
            "range-fetched {fetched:?}, stored {chunks:?}"
        );
        assert_clean(&self.s3, &self.hermetic);
    }
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_default_listing_count_and_top_match_local() {
    let parity = Parity::new(PREFIX);
    for args in [
        &[][..],
        &["--count"],
        &["--top", "2"],
        &["--top", "10"],
        &["-l", "7"],
    ] {
        parity.assert_same(args);
    }
    let (count, _, _) = parity.remote(&["--count"]);
    assert_eq!(count.trim(), "1400");
    parity.finish();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_sorted_formats_match_local() {
    let parity = Parity::new(PREFIX);
    for args in [
        &["-f", "size"][..],
        &["-f", "atime"],
        &["-f", "size", "-l", "25"],
        &["-f", "atime", "-l", "25"],
    ] {
        parity.assert_same(args);
    }
    parity.finish();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_partition_scoping_matches_local() {
    let parity = Parity::new(PREFIX);
    // Scoping reaches the bound glob: a count over one partition reads that partition's
    // chunk and no other.
    let (out, err, ok) = parity.remote(&["-u", "bob", "--count"]);
    assert!(ok, "xdu-find -u bob failed: {err}");
    assert_eq!(out.trim(), "450");
    assert_eq!(
        range_fetched_keys(&parity.s3),
        BTreeSet::from([format!("{PREFIX}/bob/000000.parquet")])
    );
    for args in [
        &["-u", "alice"][..],
        &["-u", "__root__"],
        &["-u", "bob", "--count"],
        &["-u", "__root__", "-f", "size"],
        &["-u", "alice", "--top", "5"],
    ] {
        parity.assert_same(args);
    }
    parity.finish();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_filters_match_local() {
    let parity = Parity::new(PREFIX);
    for args in [
        &["--min-size", "1K"][..],
        &["--max-size", "100"],
        &["--older-than", "30"],
        &["--newer-than", "10"],
        &["--mtime-older-than", "50"],
        &["--mtime-newer-than", "20"],
        &["-p", "*.log"],
        &["--regex", "-p", "\\.txt$"],
        &[
            "-u",
            "alice",
            "--max-size",
            "600",
            "-p",
            "*.log",
            "-f",
            "size",
        ],
        &["--min-size", "500", "-p", "*.log", "-f", "size"],
        &["--older-than", "30", "--max-size", "1200", "--count"],
    ] {
        parity.assert_same(args);
    }
    parity.finish();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_csv_and_json_match_local() {
    let parity = Parity::new(PREFIX);
    for args in [
        &["-f", "csv"][..],
        &["-f", "json"],
        &["-f", "csv", "-u", "bob"],
        &["-f", "json", "-p", "*.log", "-l", "10"],
    ] {
        parity.assert_same(args);
    }
    parity.finish();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_index_from_xdu_index_matches_local() {
    let parity = Parity::new(PREFIX);
    for args in [&["--count"][..], &["-f", "size", "-l", "5"]] {
        let (local, err, ok) = parity.local(args);
        assert!(ok, "local xdu-find failed: {err}");
        let (remote, err, ok) = run(parity
            .s3
            .find_command(&parity.hermetic)
            .env("XDU_INDEX", &parity.locator)
            .args(args));
        assert!(
            ok,
            "xdu-find with XDU_INDEX={} failed: {err}",
            parity.locator
        );
        assert!(!local.trim().is_empty());
        assert_eq!(local, remote, "{args:?}");
    }
    parity.finish();
}

/// Where the cost fixture lives, and its shape: about 16 MiB in 8 chunks over 4 partitions.
const COST_PREFIX: &str = "cost";
const COST_PARTITIONS: &[&str] = &["__root__", "alice", "bob", "carol"];
const COST_CHUNKS: usize = 2;
const COST_ROWS: usize = 9500;

/// A bucket holding the cost fixture, checked against the floor the cost bound is stated on.
fn cost_fixture() -> (TestS3, TempDir, u64, u64) {
    let s3 = TestS3::start();
    let work = TempDir::new().unwrap();
    let index = work.path().join("index");
    let rows = encoded_index(&index, COST_PARTITIONS, COST_CHUNKS, COST_ROWS);
    s3.seed_index(&index, COST_PREFIX);

    let sizes = s3.parquet_sizes(COST_PREFIX);
    let stored: u64 = sizes.values().sum();
    let partitions: BTreeSet<&str> = sizes.keys().map(|k| k.split('/').nth(1).unwrap()).collect();
    assert!(stored >= 10 << 20, "the fixture holds only {stored} bytes");
    assert!(partitions.len() >= 4, "the fixture spans {partitions:?}");
    assert!(
        sizes.len() <= 12,
        "the fixture holds {} chunks",
        sizes.len()
    );
    (s3, work, rows, stored)
}

/// Every byte the proxy sent back, and a one-line account of the requests.
fn traffic(s3: &TestS3) -> (u64, String) {
    let records = s3.requests();
    let received: u64 = records.iter().map(|r| r.bytes_to_client).sum();
    let lists = records
        .iter()
        .filter(|r| r.target.contains("list-type="))
        .count();
    let heads = records.iter().filter(|r| r.method == "HEAD").count();
    let gets = records
        .iter()
        .filter(|r| r.method == "GET" && !r.target.contains("list-type="))
        .count();
    (
        received,
        format!(
            "{} requests: LIST {lists}, HEAD {heads}, GET {gets}",
            records.len()
        ),
    )
}

/// Print a measurement past libtest's capture, so CI's log of a passing run keeps it.
fn record(line: &str) {
    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "{line}").unwrap();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_count_receives_under_five_percent_of_stored_bytes() {
    let (s3, work, rows, stored) = cost_fixture();
    let hermetic = Hermetic::new();

    let (local, err, ok) = run(hermetic
        .command("xdu-find")
        .arg("-i")
        .arg(work.path().join("index"))
        .arg("--count"));
    assert!(ok, "local xdu-find failed: {err}");
    let (out, err, ok) =
        run(s3
            .find_command(&hermetic)
            .args(["-i", &s3.url(COST_PREFIX), "--count"]));
    assert!(ok, "xdu-find --count failed: {err}");
    assert_eq!(out.trim(), rows.to_string());
    assert_eq!(out, local);

    // Every chunk was read, and only by bounded ranges.
    let chunks: BTreeSet<String> = s3.parquet_sizes(COST_PREFIX).into_keys().collect();
    assert_eq!(range_fetched_keys(&s3), chunks);
    let (received, account) = traffic(&s3);
    let percent = received as f64 * 100.0 / stored as f64;
    record(&format!(
        "s3-count-bytes: received {received} of {stored} stored bytes ({percent:.3}%), {account}"
    ));
    assert!(
        received * 20 < stored,
        "--count received {received} bytes of {stored} stored ({percent:.3}%), not under 5%"
    );
    assert_clean(&s3, &hermetic);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_unmatched_pattern_scan_receives_over_a_quarter_of_stored_bytes() {
    // The meter's negative control: a pattern no path matches makes the binary read the whole
    // path column, so a meter that saw only footers, or nothing, fails here.
    let (s3, _work, _rows, stored) = cost_fixture();
    let hermetic = Hermetic::new();
    let (out, err, ok) = run(s3.find_command(&hermetic).args([
        "-i",
        &s3.url(COST_PREFIX),
        "-p",
        "*no-such-path-anywhere*",
        "--count",
    ]));
    assert!(ok, "xdu-find failed: {err}");
    assert_eq!(out.trim(), "0");

    let (received, account) = traffic(&s3);
    let percent = received as f64 * 100.0 / stored as f64;
    record(&format!(
        "s3-scan-bytes: received {received} of {stored} stored bytes ({percent:.3}%), {account}"
    ));
    assert!(
        received * 4 > stored,
        "a full path scan received only {received} bytes of {stored} ({percent:.3}%)"
    );
    assert_clean(&s3, &hermetic);
}

/// The requests the proxy saw for one key, by method.
fn requests_for(s3: &TestS3, key: &str, method: &str) -> usize {
    s3.requests()
        .iter()
        .filter(|r| r.method == method && r.key() == key)
        .count()
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_marker_at_the_size_cap_is_read() {
    let (s3, _work) = seeded();
    let marker_key = format!("{PREFIX}/{COMPLETION_MARKER}");
    s3.seed_object(&marker_key, &padded_marker(MARKER_READ_LIMIT as usize));
    let hermetic = Hermetic::new();

    let (out, err, ok) = run(s3
        .find_command(&hermetic)
        .args(["-i", &s3.url(PREFIX), "--count"]));
    assert!(ok, "a marker of exactly the cap was refused: {err}");
    assert_eq!(out.trim(), ROWS.to_string());
    let gets: Vec<_> = s3
        .requests()
        .into_iter()
        .filter(|r| r.method == "GET" && r.key() == marker_key)
        .collect();
    assert_eq!(gets.len(), 1, "expected one GET of the marker: {gets:?}");
    assert_eq!(
        gets[0].bounded_range(),
        Some((0, MARKER_READ_LIMIT - 1)),
        "{gets:?}"
    );
    assert_clean(&s3, &hermetic);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_marker_over_the_size_cap_is_refused_unfetched() {
    let (s3, _work) = seeded();
    let marker_key = format!("{PREFIX}/{COMPLETION_MARKER}");
    s3.seed_object(&marker_key, &padded_marker(MARKER_READ_LIMIT as usize + 1));
    let hermetic = Hermetic::new();

    let (out, err, ok) = run(s3
        .find_command(&hermetic)
        .args(["-i", &s3.url(PREFIX), "--count"]));
    assert!(!ok, "a marker over the cap was accepted: {out}");
    assert!(out.is_empty(), "rows printed past an unread marker: {out}");
    assert!(
        err.contains(&MARKER_READ_LIMIT.to_string()),
        "the refusal does not name the cap: {err}"
    );
    assert!(
        requests_for(&s3, &marker_key, "HEAD") >= 1,
        "the marker's size was not asked for: {:#?}",
        s3.requests()
    );
    assert_eq!(
        requests_for(&s3, &marker_key, "GET"),
        0,
        "the oversized marker was fetched: {:#?}",
        s3.requests()
    );
    assert!(
        range_fetched_keys(&s3).is_empty(),
        "a chunk was read past the refusal"
    );
    assert_clean(&s3, &hermetic);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_failures_name_distinct_causes_and_print_no_rows() {
    let (s3, work) = seeded();
    let index = work.path().join("index");
    // The same chunks without a marker, and with a marker of a format this reader does not know.
    for file in common::list_files_recursive(&index) {
        let rel = file.strip_prefix(&index).unwrap().to_str().unwrap();
        if rel != COMPLETION_MARKER {
            s3.seed_object(&format!("nomarker/{rel}"), &fs::read(&file).unwrap());
            s3.seed_object(&format!("future/{rel}"), &fs::read(&file).unwrap());
        }
    }
    let marker = String::from_utf8(s3.object(&format!("{PREFIX}/{COMPLETION_MARKER}"))).unwrap();
    assert!(marker.contains("format=2\n"), "{marker}");
    s3.seed_object(
        &format!("future/{COMPLETION_MARKER}"),
        marker.replace("format=2\n", "format=99\n").as_bytes(),
    );

    let hermetic = Hermetic::new();
    let find = |locator: &str| {
        let mut cmd = s3.find_command(&hermetic);
        cmd.args(["-i", locator, "--count"]);
        cmd
    };
    let mut cases: Vec<(&str, &str, Command)> = Vec::new();
    let mut dead = find(&s3.url(PREFIX));
    dead.env("AWS_ENDPOINT_URL", "http://127.0.0.1:1");
    cases.push(("unreachable endpoint", "cannot reach the S3 endpoint", dead));
    let mut wrong = find(&s3.url(PREFIX));
    wrong.env("AWS_SECRET_ACCESS_KEY", WRONG_SECRET_KEY);
    cases.push(("wrong secret", "refused the credentials", wrong));
    let mut none = find(&s3.url(PREFIX));
    none.env_remove("AWS_ACCESS_KEY_ID")
        .env_remove("AWS_SECRET_ACCESS_KEY");
    cases.push(("no credentials", "no S3 credentials", none));
    cases.push((
        "missing bucket",
        "does not exist",
        find(&format!("s3://no-such-bucket/{PREFIX}")),
    ));
    cases.push((
        "missing prefix",
        "holds no objects",
        find(&s3.url("missing")),
    ));
    cases.push((
        "missing marker",
        "carries no index format version",
        find(&s3.url("nomarker")),
    ));
    cases.push((
        "unknown format",
        "index format version 99",
        find(&s3.url("future")),
    ));

    let needles: Vec<&str> = cases.iter().map(|(_, needle, _)| *needle).collect();
    for (case, needle, mut cmd) in cases {
        let (out, err, ok) = run(&mut cmd);
        assert!(!ok, "{case}: xdu-find succeeded");
        assert!(out.is_empty(), "{case}: rows printed: {out}");
        assert!(
            err.contains(needle),
            "{case}: stderr does not say '{needle}': {err}"
        );
        for other in needles.iter().filter(|n| **n != needle) {
            assert!(
                !err.contains(other),
                "{case}: stderr also says '{other}', so the causes are not distinct: {err}"
            );
        }
    }
    assert!(
        range_fetched_keys(&s3).is_empty(),
        "a chunk was read in a failing case"
    );
    assert_clean(&s3, &hermetic);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_prefix_with_a_quote_matches_local() {
    // A quote in the prefix would end a string literal in SQL text; bound, it is just a byte.
    let parity = Parity::new("it's/idx");
    for args in [
        &[][..],
        &["--count"],
        &["-u", "alice", "-f", "size"],
        &["--top", "3"],
    ] {
        parity.assert_same(args);
    }
    assert!(
        parity
            .s3
            .requests()
            .iter()
            .any(|r| r.target.contains("prefix=it%27s%2Fidx%2F")),
        "no listing of the quoted prefix reached the endpoint: {:#?}",
        parity.s3.requests()
    );
    parity.finish();
}

/// Write the hermetic child's shared credentials and config files.
fn write_aws_files(hermetic: &Hermetic, credentials: &str, config: &str) {
    fs::write(hermetic.credentials_file(), credentials).unwrap();
    fs::write(hermetic.config_file(), config).unwrap();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_credentials_from_a_profile_in_the_shared_files() {
    let (s3, _work) = seeded();
    let hermetic = Hermetic::new();
    write_aws_files(
        &hermetic,
        &format!(
            "[default]\naws_access_key_id = {ACCESS_KEY}\naws_secret_access_key = {WRONG_SECRET_KEY}\n\n\
             [xdu-reader]\naws_access_key_id = {ACCESS_KEY}\naws_secret_access_key = {SECRET_KEY}\n"
        ),
        "[profile xdu-reader]\nregion = us-east-1\n",
    );
    let profile_find = |profile: Option<&str>| {
        let mut cmd = hermetic.command("xdu-find");
        cmd.env("XDU_EXTENSION_DIR", s3.extension_dir())
            .env("AWS_ENDPOINT_URL", s3.proxy_url())
            .args(["-i", &s3.url(PREFIX), "--count"]);
        if let Some(profile) = profile {
            cmd.env("AWS_PROFILE", profile);
        }
        run(&mut cmd)
    };

    let (out, err, ok) = profile_find(Some("xdu-reader"));
    assert!(ok, "the profile's credentials were not used: {err}");
    assert_eq!(out.trim(), ROWS.to_string());
    // Control: without AWS_PROFILE the default profile, which holds the wrong secret, is used.
    let (out, err, ok) = profile_find(None);
    assert!(
        !ok && out.is_empty(),
        "the default profile's wrong secret was accepted"
    );
    assert!(err.contains("refused the credentials"), "{err}");
    assert_clean(&s3, &hermetic);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_endpoint_from_a_profile_services_section() {
    let (s3, _work) = seeded();
    let hermetic = Hermetic::new();
    write_aws_files(
        &hermetic,
        "",
        &format!(
            "[default]\nservices = xdu-local\n\n[services xdu-local]\ns3 =\n  endpoint_url = {}\n",
            s3.proxy_url()
        ),
    );
    let (out, err, ok) = run(hermetic
        .command("xdu-find")
        .env("XDU_EXTENSION_DIR", s3.extension_dir())
        .env("AWS_ACCESS_KEY_ID", ACCESS_KEY)
        .env("AWS_SECRET_ACCESS_KEY", SECRET_KEY)
        .args(["-i", &s3.url(PREFIX), "--count"]));
    assert!(ok, "xdu-find failed: {err}");
    assert_eq!(out.trim(), ROWS.to_string());
    // The endpoint came from the services section: the read went through the proxy.
    let chunks: BTreeSet<String> = s3.parquet_sizes(PREFIX).into_keys().collect();
    assert_eq!(range_fetched_keys(&s3), chunks);
    assert_clean(&s3, &hermetic);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn find_tolerated_errors_marker_warns_as_local() {
    let (s3, work) = seeded();
    let index = work.path().join("index");
    let marker = fs::read_to_string(index.join(COMPLETION_MARKER)).unwrap();
    assert!(marker.contains("errors=0\n"), "{marker}");
    let marker = marker.replace("errors=0\n", "errors=3\n");
    fs::write(index.join(COMPLETION_MARKER), &marker).unwrap();
    s3.seed_object(&format!("{PREFIX}/{COMPLETION_MARKER}"), marker.as_bytes());
    let hermetic = Hermetic::new();

    let canonical = index.canonicalize().unwrap();
    let (local, local_err, ok) = run(hermetic
        .command("xdu-find")
        .arg("-i")
        .arg(&canonical)
        .arg("--count"));
    assert!(ok, "local xdu-find failed: {local_err}");
    assert!(local_err.contains("3 tolerated error(s)"), "{local_err}");
    let (remote, remote_err, ok) =
        run(s3
            .find_command(&hermetic)
            .args(["-i", &s3.url(PREFIX), "--count"]));
    assert!(ok, "xdu-find failed: {remote_err}");
    assert_eq!(remote, local);
    assert_eq!(
        remote_err,
        local_err.replace(canonical.to_str().unwrap(), &s3.url(PREFIX)),
        "the S3 warning differs from the local one"
    );
    assert_clean(&s3, &hermetic);
}

// =============================================================================
// xdu -o s3:// builds the index into the bucket
// =============================================================================

/// Where the writer tests build their index in the bucket.
const WRITE_PREFIX: &str = "built/idx";
/// Records per chunk in the writer tests: small, so a partition spans several chunks.
const WRITE_BUFFSIZE: &str = "40";

/// One row of an index in schema order.
type IndexRow = (String, i64, i64, i64, i64, i64, i64, i64);

/// `xdu`'s arguments for a build of `source` into `out`, `WRITE_BUFFSIZE` records a chunk.
fn xdu_args(source: &Path, out: &str, jobs: &str) -> Vec<OsString> {
    xdu_args_chunked(source, out, jobs, WRITE_BUFFSIZE)
}

/// `xdu`'s arguments for a build of `source` into `out`, `buffsize` records a chunk.
fn xdu_args_chunked(source: &Path, out: &str, jobs: &str, buffsize: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["--apparent-size", "-j", jobs, "-B", buffsize]
        .iter()
        .map(OsString::from)
        .collect();
    args.push(source.as_os_str().to_owned());
    args.push("-o".into());
    args.push(out.into());
    args
}

/// Files of distinct sizes under each `(directory, count)`; `""` is the source root itself.
fn small_tree(root: &Path, groups: &[(&str, usize)]) {
    let mut size = 1;
    for (dir, count) in groups {
        for i in 0..*count {
            create_test_file(&root.join(dir).join(format!("f{i:04}.dat")), size).unwrap();
            size += 1;
        }
    }
}

/// Every file under `dir`, relative to it.
fn relative_files(dir: &Path) -> BTreeSet<String> {
    common::list_files_recursive(dir)
        .into_iter()
        .map(|f| f.strip_prefix(dir).unwrap().to_str().unwrap().to_string())
        .collect()
}

/// Every row of the chunks in `dir`, all eight columns, sorted by path.
fn partition_rows(dir: &Path) -> Vec<IndexRow> {
    let conn = xdu::engine::open_connection().unwrap();
    let glob = format!("{}/*.parquet", dir.display());
    let mut stmt = conn
        .prepare("SELECT * FROM read_parquet(?) ORDER BY path")
        .unwrap();
    stmt.query_map([glob], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
        ))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

/// A chunk's Parquet schema, key-value metadata and writer, as comparable text.
fn chunk_metadata(file: &Path) -> String {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let reader = SerializedFileReader::new(fs::File::open(file).unwrap()).unwrap();
    let meta = reader.metadata().file_metadata();
    format!(
        "{:?}\n{:?}\n{:?}",
        meta.schema_descr().root_schema(),
        meta.key_value_metadata(),
        meta.created_by()
    )
}

/// A marker body without its `completed_at` line, which differs between any two runs.
fn marker_without_time(body: &[u8]) -> String {
    String::from_utf8(body.to_vec())
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with("completed_at="))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn is_list(record: &ProxyRecord) -> bool {
    record.method == "GET" && record.target.contains("list-type=2")
}

fn is_chunk_put(record: &ProxyRecord) -> bool {
    record.method == "PUT" && record.key().ends_with(".parquet")
}

fn marker_key(prefix: &str) -> String {
    format!("{prefix}/{COMPLETION_MARKER}")
}

/// The proxy saw no DELETE and no PUT: nothing in the bucket was written or removed.
fn assert_no_write(s3: &TestS3, case: &str) {
    let writes: Vec<ProxyRecord> = s3
        .requests()
        .into_iter()
        .filter(|r| r.method == "DELETE" || r.method == "PUT" || r.method == "POST")
        .collect();
    assert!(
        writes.is_empty(),
        "{case}: the bucket was written: {writes:#?}"
    );
}

/// A bucket holding an index `xdu` built locally under `WRITE_PREFIX`, and its stored bytes.
fn seeded_write_target(work: &Path) -> (TestS3, BTreeMap<String, Vec<u8>>) {
    let s3 = TestS3::start();
    let source = work.join("old-source");
    let index = work.join("old-index");
    small_tree(&source, &[("", 3), ("alice", 90), ("bob", 50)]);
    build_index(&source, &index);
    s3.seed_index(&index, WRITE_PREFIX);
    let before = s3.snapshot(WRITE_PREFIX);
    assert!(before.contains_key(COMPLETION_MARKER), "{before:?}");
    (s3, before)
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_round_trip_matches_a_local_run() {
    let s3 = TestS3::start();
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    varied_source_tree(&source);
    let source_before = relative_files(&source);
    let local = work.path().join("local");
    let hermetic = Hermetic::new();

    let (_, err, ok) =
        run(hermetic
            .command("xdu")
            .args(xdu_args(&source, local.to_str().unwrap(), "4")));
    assert!(ok, "the local build failed: {err}");
    let (out, err, ok) =
        run(s3
            .write_command(&hermetic)
            .args(xdu_args(&source, &s3.url(WRITE_PREFIX), "4")));
    assert!(ok, "the S3 build failed: {err}");
    assert!(out.is_empty(), "xdu printed to stdout: {out}");
    assert!(
        err.contains(&format!(
            "Marker {} written",
            s3.url(&marker_key(WRITE_PREFIX))
        )),
        "the log does not name the S3 marker: {err}"
    );

    // The same objects at the same relative keys, the loose-file partition included.
    let files = relative_files(&local);
    assert_eq!(s3.keys(WRITE_PREFIX), files);
    assert!(
        files.iter().any(|k| k.starts_with("__root__/")),
        "{files:?}"
    );
    assert!(files.contains("alice/000001.parquet"), "{files:?}");

    // The same rows in every column, and chunks encoded the same way.
    let partitions: BTreeSet<&str> = files
        .iter()
        .filter_map(|k| k.split_once('/').map(|(p, _)| p))
        .collect();
    assert_eq!(partitions.len(), 3, "{partitions:?}");
    for partition in &partitions {
        let local_rows = partition_rows(&local.join(partition));
        assert!(!local_rows.is_empty(), "{partition} holds no rows");
        assert_eq!(
            partition_rows(&s3.object_dir(WRITE_PREFIX).join(partition)),
            local_rows,
            "partition {partition} differs"
        );
    }
    for key in files.iter().filter(|k| k.ends_with(".parquet")) {
        assert_eq!(
            chunk_metadata(&s3.object_dir(WRITE_PREFIX).join(key)),
            chunk_metadata(&local.join(key)),
            "{key} was encoded differently"
        );
    }
    assert_eq!(
        marker_without_time(&s3.object(&marker_key(WRITE_PREFIX))),
        marker_without_time(&fs::read(local.join(COMPLETION_MARKER)).unwrap())
    );

    // Nothing written locally: not under HOME, TMPDIR or the working directory, where a
    // misread `s3://` would have created `s3:/…`, and not in the source tree.
    hermetic.assert_left_empty();
    assert_eq!(relative_files(&source), source_before);
    assert!(
        !s3.requests()
            .iter()
            .any(|r| r.method == "GET" && !is_list(r)),
        "the writer read an object back"
    );
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_index_answers_xdu_find_as_a_local_one_does() {
    let s3 = TestS3::start();
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    varied_source_tree(&source);
    let local = work.path().join("local");
    let hermetic = Hermetic::new();
    // One chunk a partition, each larger than the footer read a query opens it with, so a
    // ranged read never has to cover a whole chunk.
    let (_, err, ok) = run(hermetic.command("xdu").args(xdu_args_chunked(
        &source,
        local.to_str().unwrap(),
        "2",
        "100000",
    )));
    assert!(ok, "the local build failed: {err}");
    let (_, err, ok) = run(s3.write_command(&hermetic).args(xdu_args_chunked(
        &source,
        &s3.url(WRITE_PREFIX),
        "2",
        "100000",
    )));
    assert!(ok, "the S3 build failed: {err}");

    for args in [
        &["--count"][..],
        &["-u", "alice", "--count"],
        &["-u", "__root__", "--count"],
        &["--top", "3"],
        &["-f", "size"],
        &["-f", "size", "-u", "bob", "-l", "20"],
    ] {
        let (want, err, ok) = run(hermetic
            .command("xdu-find")
            .arg("-i")
            .arg(&local)
            .args(args));
        assert!(ok, "local xdu-find {args:?} failed: {err}");
        assert!(
            !want.trim().is_empty(),
            "local xdu-find {args:?} printed nothing"
        );
        let (got, err, ok) = run(s3
            .find_command(&hermetic)
            .args(["-i", &s3.url(WRITE_PREFIX)])
            .args(args));
        assert!(ok, "xdu-find {args:?} over the written index failed: {err}");
        assert_eq!(got, want, "xdu-find {args:?}");
    }
    assert_clean(&s3, &hermetic);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_requests_list_then_clear_then_chunks_and_the_marker_last() {
    let (s3, _) = seeded_write_target(TempDir::new().unwrap().path());
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    small_tree(
        &source,
        &[("", 5), ("alice", 130), ("bob", 70), ("carol", 45)],
    );
    let hermetic = Hermetic::new();
    let (_, err, ok) =
        run(s3
            .write_command(&hermetic)
            .args(xdu_args(&source, &s3.url(WRITE_PREFIX), "1")));
    assert!(ok, "the S3 build failed: {err}");

    let requests = s3.requests();
    let marker = marker_key(WRITE_PREFIX);
    assert!(
        is_list(&requests[0]) && requests[0].target.contains("prefix=built%2Fidx%2F"),
        "the run did not open with a listing of the prefix: {requests:#?}"
    );
    assert!(
        requests[1].method == "DELETE" && requests[1].key() == marker,
        "the marker was not cleared right after the listing: {requests:#?}"
    );
    let puts: Vec<usize> = (0..requests.len())
        .filter(|&i| is_chunk_put(&requests[i]))
        .collect();
    assert!(puts.len() >= 8, "{} chunk PUTs: {requests:#?}", puts.len());
    let last = requests.last().unwrap();
    assert!(
        last.method == "PUT" && last.key() == marker && last.status == 200,
        "the marker was not the last write: {requests:#?}"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.method == "PUT" && r.key() == marker)
            .count(),
        1,
        "the marker was written more than once"
    );
    // Chunks go up as partitions finalize: each partition's prune listing comes between its
    // chunk PUTs and the next partition's, not after every chunk of the run.
    let prune_lists = (puts[0]..*puts.last().unwrap())
        .filter(|&i| is_list(&requests[i]))
        .count();
    assert_eq!(prune_lists, 3, "{requests:#?}");
    assert!(requests.iter().all(|r| r.status < 300), "{requests:#?}");
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_rerun_with_fewer_files_leaves_no_stale_chunk() {
    let s3 = TestS3::start();
    let work = TempDir::new().unwrap();
    let big = work.path().join("big");
    let small = work.path().join("small");
    small_tree(&big, &[("", 50), ("alice", 130), ("bob", 90)]);
    small_tree(&small, &[("", 2), ("alice", 30), ("bob", 50)]);
    let local = work.path().join("local");
    let hermetic = Hermetic::new();

    for source in [&big, &small] {
        let (_, err, ok) =
            run(s3
                .write_command(&hermetic)
                .args(xdu_args(source, &s3.url(WRITE_PREFIX), "2")));
        assert!(ok, "the S3 build of {} failed: {err}", source.display());
        let (_, err, ok) =
            run(hermetic
                .command("xdu")
                .args(xdu_args(source, local.to_str().unwrap(), "2")));
        assert!(ok, "the local build of {} failed: {err}", source.display());
        if source == &big {
            // The first run wrote several chunks in every partition the second one shrinks.
            for partition in ["__root__", "alice", "bob"] {
                let chunks = s3
                    .keys(WRITE_PREFIX)
                    .iter()
                    .filter(|k| k.starts_with(&format!("{partition}/")))
                    .count();
                assert!(chunks > 1, "{partition} holds {chunks} chunk(s)");
            }
        }
    }

    let keys = s3.keys(WRITE_PREFIX);
    assert_eq!(keys, relative_files(&local));
    let expected: BTreeSet<String> = [
        COMPLETION_MARKER,
        "__root__/000000.parquet",
        "alice/000000.parquet",
        "bob/000000.parquet",
        "bob/000001.parquet",
    ]
    .iter()
    .map(|k| k.to_string())
    .collect();
    assert_eq!(keys, expected);
    for partition in ["__root__", "alice", "bob"] {
        assert_eq!(
            partition_rows(&s3.object_dir(WRITE_PREFIX).join(partition)),
            partition_rows(&local.join(partition)),
            "{partition}"
        );
    }
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_prune_removes_stale_chunks_past_a_gap() {
    let work = TempDir::new().unwrap();
    let (s3, _) = seeded_write_target(work.path());
    // A foreign edit left chunks beyond a gap; a local prune stops at the gap, this one may
    // not, since every key it lists is a stale chunk of the partition.
    for key in [
        "alice/000005.parquet",
        "alice/000009.parquet",
        "bob/000004.parquet",
    ] {
        s3.seed_object(&format!("{WRITE_PREFIX}/{key}"), b"stale");
    }
    let source = work.path().join("source");
    small_tree(&source, &[("alice", 30), ("bob", 20)]);
    let hermetic = Hermetic::new();
    let (_, err, ok) =
        run(s3
            .write_command(&hermetic)
            .args(xdu_args(&source, &s3.url(WRITE_PREFIX), "1")));
    assert!(ok, "the S3 build failed: {err}");

    let chunks: Vec<String> = s3
        .keys(WRITE_PREFIX)
        .into_iter()
        .filter(|k| k.ends_with(".parquet"))
        .collect();
    // __root__ is not in this source, so its old chunk stays, as it would locally.
    assert_eq!(
        chunks,
        [
            "__root__/000000.parquet",
            "alice/000000.parquet",
            "bob/000000.parquet"
        ]
    );
    assert!(err.contains("pruned 3 stale"), "{err}");
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_missing_source_or_reserved_name_is_refused_before_the_clear() {
    let work = TempDir::new().unwrap();
    let (s3, before) = seeded_write_target(work.path());
    let hermetic = Hermetic::new();
    let reserved = work.path().join("reserved");
    small_tree(&reserved, &[("alice", 3), ("__root__", 2)]);
    let marker_dir = work.path().join("marker-dir");
    small_tree(&marker_dir, &[("alice", 3), (COMPLETION_MARKER, 2)]);

    for (case, source, needle) in [
        (
            "missing source",
            work.path().join("no-such-source"),
            "Failed to resolve directory",
        ),
        ("reserved partition", reserved, "'__root__'"),
        ("reserved marker name", marker_dir, "'.xdu-complete'"),
    ] {
        let (out, err, ok) =
            run(s3
                .write_command(&hermetic)
                .args(xdu_args(&source, &s3.url(WRITE_PREFIX), "2")));
        assert!(!ok, "{case}: xdu succeeded");
        assert!(out.is_empty(), "{case}: {out}");
        assert!(
            err.contains(needle),
            "{case}: stderr does not say {needle}: {err}"
        );
        assert_no_write(&s3, case);
        assert_eq!(
            s3.snapshot(WRITE_PREFIX),
            before,
            "{case}: the index changed"
        );
    }
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_unsafe_partition_name_is_refused_before_the_clear() {
    use std::os::unix::ffi::OsStrExt;

    let work = TempDir::new().unwrap();
    let (s3, before) = seeded_write_target(work.path());
    let hermetic = Hermetic::new();
    let mut names: Vec<OsString> = ["a~b", "50%", "x#y", "caf\u{e9}"]
        .iter()
        .map(OsString::from)
        .collect();
    // APFS refuses a name that is not UTF-8, so that case runs where the filesystem has one
    // (the Linux CI leg).
    let probe = work.path().join("probe");
    fs::create_dir(&probe).unwrap();
    let raw = std::ffi::OsStr::from_bytes(b"bad\xffname");
    let non_utf8_possible = fs::create_dir(probe.join(raw)).is_ok();
    if non_utf8_possible {
        names.push(raw.to_owned());
    }
    record(&format!(
        "write-unsafe-names: {} case(s), non-UTF-8 case {}",
        names.len(),
        if non_utf8_possible {
            "ran"
        } else {
            "skipped: the filesystem refuses the name"
        }
    ));

    for (i, name) in names.iter().enumerate() {
        let source = work.path().join(format!("source-{i}"));
        small_tree(&source, &[("alice", 3), ("bob", 2)]);
        fs::create_dir(source.join(name)).unwrap();
        create_test_file(&source.join(name).join("f.dat"), 10).unwrap();
        let case = name.to_string_lossy().to_string();
        // A partition filter that leaves the directory out does not save the run: the check
        // covers every top-level directory, as the reserved-name check does.
        for filter in [None, Some("alice")] {
            let mut cmd = s3.write_command(&hermetic);
            cmd.args(xdu_args(&source, &s3.url(WRITE_PREFIX), "2"));
            if let Some(filter) = filter {
                cmd.args(["-p", filter]);
            }
            let (out, err, ok) = run(&mut cmd);
            assert!(!ok, "{case} ({filter:?}): xdu succeeded");
            assert!(out.is_empty(), "{case}: {out}");
            assert!(
                err.contains(&format!("'{case}'"))
                    && err.contains("cannot be a partition of an S3 index"),
                "{case}: stderr does not name the directory: {err}"
            );
            assert_no_write(&s3, &case);
            assert_eq!(
                s3.snapshot(WRITE_PREFIX),
                before,
                "{case}: the index changed"
            );
        }
    }
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_endpoint_and_credential_failures_leave_the_index_untouched() {
    let work = TempDir::new().unwrap();
    let (s3, before) = seeded_write_target(work.path());
    let source = work.path().join("source");
    small_tree(&source, &[("alice", 5)]);
    let hermetic = Hermetic::new();
    let write = |locator: &str| {
        let mut cmd = s3.write_command(&hermetic);
        cmd.args(xdu_args(&source, locator, "2"));
        cmd
    };

    let mut cases: Vec<(&str, &str, Command)> = Vec::new();
    let mut wrong = write(&s3.url(WRITE_PREFIX));
    wrong.env("AWS_SECRET_ACCESS_KEY", WRONG_SECRET_KEY);
    cases.push(("wrong secret", "refused the credentials", wrong));
    let mut dead = write(&s3.url(WRITE_PREFIX));
    dead.env("AWS_ENDPOINT_URL", "http://127.0.0.1:1");
    cases.push(("unreachable endpoint", "cannot reach the S3 endpoint", dead));
    cases.push((
        "missing bucket",
        "does not exist",
        write(&format!("s3://no-such-bucket/{WRITE_PREFIX}")),
    ));
    let mut none = write(&s3.url(WRITE_PREFIX));
    none.env_remove("AWS_ACCESS_KEY_ID")
        .env_remove("AWS_SECRET_ACCESS_KEY");
    cases.push(("no credentials", "no S3 credentials", none));

    let needles: Vec<&str> = cases.iter().map(|(_, needle, _)| *needle).collect();
    for (case, needle, mut cmd) in cases {
        let started = std::time::Instant::now();
        let (out, err, ok) = run(&mut cmd);
        assert!(!ok, "{case}: xdu succeeded");
        assert!(out.is_empty(), "{case}: {out}");
        assert!(
            err.contains(needle),
            "{case}: stderr does not say '{needle}': {err}"
        );
        for other in needles.iter().filter(|n| **n != needle) {
            assert!(
                !err.contains(other),
                "{case}: stderr also says '{other}': {err}"
            );
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "{case}: took {:?} to fail",
            started.elapsed()
        );
        assert_no_write(&s3, case);
        assert_eq!(
            s3.snapshot(WRITE_PREFIX),
            before,
            "{case}: the index changed"
        );
    }
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_read_only_credentials_leave_the_marker_in_place() {
    let work = TempDir::new().unwrap();
    let (s3, before) = seeded_write_target(work.path());
    let source = work.path().join("source");
    small_tree(&source, &[("alice", 5)]);
    let hermetic = Hermetic::new();
    let (out, err, ok) = run(s3
        .write_command(&hermetic)
        .env("AWS_ACCESS_KEY_ID", READ_ONLY_ACCESS_KEY)
        .env("AWS_SECRET_ACCESS_KEY", READ_ONLY_SECRET_KEY)
        .args(xdu_args(&source, &s3.url(WRITE_PREFIX), "2")));
    assert!(!ok, "xdu succeeded with read-only credentials");
    assert!(out.is_empty(), "{out}");
    assert!(
        err.contains("refused the credentials while removing the completion marker"),
        "{err}"
    );

    // The listing passed, so the identity reached the bucket; the marker DELETE was the first
    // write, and it was refused with the marker still there.
    let requests = s3.requests();
    assert!(
        is_list(&requests[0]) && requests[0].status == 200,
        "{requests:#?}"
    );
    let writes: Vec<&ProxyRecord> = requests.iter().filter(|r| r.method != "GET").collect();
    assert_eq!(writes.len(), 1, "{requests:#?}");
    assert_eq!(writes[0].method, "DELETE");
    assert_eq!(writes[0].key(), marker_key(WRITE_PREFIX));
    assert_eq!(writes[0].status, 403);
    assert_eq!(s3.snapshot(WRITE_PREFIX), before, "the index changed");
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_unreadable_subtree_fails_without_a_marker() {
    use std::os::unix::fs::PermissionsExt;

    let work = TempDir::new().unwrap();
    let (s3, _) = seeded_write_target(work.path());
    let source = work.path().join("source");
    small_tree(&source, &[("alice", 90), ("alice/locked", 3), ("bob", 10)]);
    let locked = source.join("alice/locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let hermetic = Hermetic::new();
    let (out, err, ok) =
        run(s3
            .write_command(&hermetic)
            .args(xdu_args(&source, &s3.url(WRITE_PREFIX), "1")));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(!ok, "xdu succeeded over an unreadable subtree");
    assert!(out.is_empty(), "{out}");
    assert!(
        err.contains(locked.to_str().unwrap()),
        "the error names no path: {err}"
    );
    assert!(err.contains("unreadable path"), "{err}");
    assert!(
        !s3.keys(WRITE_PREFIX).contains(COMPLETION_MARKER),
        "a failed run left a marker"
    );
    let requests = s3.requests();
    let cleared = requests
        .iter()
        .position(|r| r.method == "DELETE" && r.key() == marker_key(WRITE_PREFIX))
        .expect("the marker was never cleared");
    assert!(
        requests[cleared..]
            .iter()
            .any(|r| is_chunk_put(r) && r.status == 200),
        "no chunk was sent before the run failed: {requests:#?}"
    );
    assert!(
        !requests
            .iter()
            .any(|r| r.method == "PUT" && r.key() == marker_key(WRITE_PREFIX)),
        "the marker was written on a failing run"
    );
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_refused_chunk_mid_run_writes_no_marker() {
    let work = TempDir::new().unwrap();
    let (s3, _) = seeded_write_target(work.path());
    let source = work.path().join("source");
    // With one driver the partitions go in order: __root__, alpha, the refused one, zulu.
    small_tree(
        &source,
        &[
            ("", 3),
            ("alpha", 100),
            (REFUSED_PARTITION, 10),
            ("zulu", 10),
        ],
    );
    let hermetic = Hermetic::new();
    let (out, err, ok) =
        run(s3
            .write_command(&hermetic)
            .args(xdu_args(&source, &s3.url(WRITE_PREFIX), "1")));
    assert!(!ok, "xdu succeeded with a chunk refused");
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("while writing chunk"), "{err}");
    assert!(err.contains(REFUSED_PARTITION), "{err}");

    let requests = s3.requests();
    let refused = requests
        .iter()
        .position(|r| r.status == 403)
        .expect("no request was refused");
    assert!(is_chunk_put(&requests[refused]), "{requests:#?}");
    let sent_before: Vec<String> = requests[..refused]
        .iter()
        .filter(|r| is_chunk_put(r) && r.status == 200)
        .map(|r| r.key())
        .collect();
    assert!(
        sent_before.iter().filter(|k| k.contains("/alpha/")).count() >= 3,
        "alpha's chunks were not sent before the failure: {sent_before:?}"
    );
    assert!(
        !requests.iter().any(|r| r.key().contains("/zulu/")),
        "a partition queued behind the failure was written"
    );
    assert!(
        !s3.keys(WRITE_PREFIX).contains(COMPLETION_MARKER),
        "a failed run left a marker"
    );
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_secret_reaches_no_argument_output_request_or_object() {
    let s3 = TestS3::start();
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    small_tree(&source, &[("", 4), ("alice", 60)]);
    let hermetic = Hermetic::new();

    let mut good = s3.write_command(&hermetic);
    good.args(xdu_args(&source, &s3.url(WRITE_PREFIX), "2"));
    let mut refused = s3.write_command(&hermetic);
    refused
        .env("AWS_SECRET_ACCESS_KEY", WRONG_SECRET_KEY)
        .args(xdu_args(&source, &s3.url("other"), "2"));
    for (case, cmd, succeeds) in [
        ("success", &mut good, true),
        ("refusal", &mut refused, false),
    ] {
        let argv: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_no_secret(&format!("{case} argv"), &argv.join(" "));
        // `run` asserts that stdout and stderr hold no secret.
        let (_, err, ok) = run(cmd);
        assert_eq!(ok, succeeds, "{case}: {err}");
    }
    for record in s3.requests() {
        assert_no_secret("a request target", &record.target);
    }
    let objects = s3.snapshot(WRITE_PREFIX);
    assert!(
        objects.contains_key(COMPLETION_MARKER),
        "{:?}",
        objects.keys()
    );
    for (key, bytes) in &objects {
        for secret in [SECRET_KEY, WRONG_SECRET_KEY] {
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                "{key} holds a secret"
            );
        }
    }
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_and_read_through_one_profile_in_the_shared_files() {
    let s3 = TestS3::start();
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    // Wide names, one chunk: the read back must not cover a whole chunk.
    wide_source_tree(&source, "alice", ROWS);
    let hermetic = Hermetic::new();
    write_aws_files(
        &hermetic,
        &format!(
            "[default]\naws_access_key_id = {ACCESS_KEY}\naws_secret_access_key = {WRONG_SECRET_KEY}\n\n\
             [xdu-builder]\naws_access_key_id = {ACCESS_KEY}\naws_secret_access_key = {SECRET_KEY}\n"
        ),
        &format!(
            "[profile xdu-builder]\nservices = xdu-local\n\n[services xdu-local]\ns3 =\n  endpoint_url = {}\n",
            s3.proxy_url()
        ),
    );
    // No AWS_* variable but the profile: the writer and the reader resolve the same identity
    // and endpoint from the same files.
    let (_, err, ok) = run(hermetic
        .command("xdu")
        .env("AWS_PROFILE", "xdu-builder")
        .args(xdu_args_chunked(
            &source,
            &s3.url(WRITE_PREFIX),
            "2",
            "100000",
        )));
    assert!(ok, "the S3 build from the profile failed: {err}");
    assert!(s3.keys(WRITE_PREFIX).contains(COMPLETION_MARKER));
    let (out, err, ok) = run(hermetic
        .command("xdu-find")
        .env("AWS_PROFILE", "xdu-builder")
        .env("XDU_EXTENSION_DIR", s3.extension_dir())
        .args(["-i", &s3.url(WRITE_PREFIX), "--count"]));
    assert!(ok, "xdu-find from the profile failed: {err}");
    assert_eq!(out.trim(), ROWS.to_string());
    assert!(!s3.requests().is_empty(), "nothing went through the proxy");
    assert_clean(&s3, &hermetic);
}

#[test]
#[ignore = "needs the pinned httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]
fn write_absent_prefix_or_bucket_root_starts_a_new_index() {
    let s3 = TestS3::start();
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    small_tree(&source, &[("", 2), ("alice", 3)]);
    let hermetic = Hermetic::new();
    let expected: BTreeSet<String> = [
        COMPLETION_MARKER,
        "__root__/000000.parquet",
        "alice/000000.parquet",
    ]
    .iter()
    .map(|k| k.to_string())
    .collect();

    // A prefix holding nothing yet, several levels deep.
    let (_, err, ok) =
        run(s3
            .write_command(&hermetic)
            .args(xdu_args(&source, &s3.url("fresh/deep/idx"), "2")));
    assert!(ok, "a build into an absent prefix failed: {err}");
    assert_eq!(s3.keys("fresh/deep/idx"), expected);

    // The bucket's root, with and without the trailing slash.
    for locator in [format!("s3://{BUCKET}"), format!("s3://{BUCKET}/")] {
        let (_, err, ok) = run(s3
            .write_command(&hermetic)
            .args(xdu_args(&source, &locator, "2")));
        assert!(ok, "a build into {locator} failed: {err}");
    }
    let root: BTreeSet<String> = s3
        .keys("")
        .into_iter()
        .filter(|k| !k.starts_with("fresh/"))
        .collect();
    assert_eq!(root, expected);
    hermetic.assert_left_empty();
    s3.assert_proxy_clean();
}
