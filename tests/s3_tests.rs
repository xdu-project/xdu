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

use std::collections::BTreeSet;
use std::fs;

use duckdb::Connection;
use tempfile::TempDir;

use common::build_index;
use common::create_test_file;
use common::s3::{
    ACCESS_KEY, EXTENSION_DIR_VAR, Hermetic, SECRET_KEY, SESSION_TOKEN, TestS3, WRONG_SECRET_KEY,
    assert_no_secret, output_of, resolve_settings, test_extension_dir, wide_source_tree,
};
use xdu::engine::open_s3_connection_from;

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
