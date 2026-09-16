//! What reaches DuckDB as SQL text, and what does not. Establishes claims 7, 10 and 11
//! of `research/duckdb-native-s3-reads.md`, plus the two open items it records: the
//! unbounded marker read, and `PROVIDER credential_chain` needing a second extension.
//!
//! Needs a MinIO endpoint holding the fixtures from `fixtures.rs`.
//! See `bench/spikes/README.md`. It is not part of `cargo test`.

use duckdb::{Config, Connection};

const EP: &str = "127.0.0.1:19000";

fn say(label: &str, r: duckdb::Result<()>) {
    match r {
        Ok(()) => println!("{label} -> OK"),
        Err(e) => println!("{label} -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
    }
}

fn scalar(c: &Connection, sql: &str) -> String {
    c.query_row(sql, [], |r| {
        r.get::<_, String>(0)
            .or_else(|_| r.get::<_, i64>(0).map(|v| v.to_string()))
    })
    .unwrap_or_else(|e| format!("ERR: {}", e.to_string().lines().next().unwrap_or("")))
}

fn secret_sql() -> String {
    format!(
        "CREATE SECRET s (TYPE s3, KEY_ID 'probekey', SECRET 'probesecret', REGION 'us-east-1', \
         ENDPOINT '{EP}', URL_STYLE 'path', USE_SSL false)"
    )
}

/// Claim 10: the index location can be a bound parameter, so it never becomes SQL text.
/// This is the seam invariant 5 records as still interpolated raw today.
#[test]
fn claim_10_read_parquet_takes_a_bound_parameter() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("INSTALL httpfs").ok();
    c.execute_batch("LOAD httpfs").unwrap();
    c.execute_batch(&secret_sql()).unwrap();

    match c.query_row(
        "SELECT count(*) FROM read_parquet(?)",
        ["s3://xdu-probe/big/alice/*.parquet"],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(v) => println!("read_parquet(?)            -> OK, rows={v}"),
        Err(e) => println!("read_parquet(?)            -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
    }

    // The shape a reader would actually use: glob bound, numeric filter bound.
    match c.prepare("SELECT count(*) FROM read_parquet(?) WHERE size > ?") {
        Ok(mut s) => match s.query_row(
            duckdb::params!["s3://xdu-probe/big/*/*.parquet", 29_999_990i64],
            |r| r.get::<_, i64>(0),
        ) {
            Ok(v) => println!("prepared read_parquet(?)   -> OK, rows={v}"),
            Err(e) => println!("prepared                   -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
        },
        Err(e) => println!("prepare                    -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
    }
}

/// Claim 11: credentials can be set through the C config API, so no secret is ever
/// part of a SQL string that might be logged.
#[test]
fn claim_11_credentials_via_config_api_never_touch_sql() {
    let built = Config::default()
        .with("s3_access_key_id", "probekey")
        .and_then(|c| c.with("s3_secret_access_key", "probesecret"))
        .and_then(|c| c.with("s3_endpoint", EP))
        .and_then(|c| c.with("s3_url_style", "path"))
        .and_then(|c| c.with("s3_use_ssl", "false"))
        .and_then(|c| c.with("s3_region", "us-east-1"));

    match built {
        Err(e) => println!("Config::with(s3_*) -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
        Ok(cfg) => match Connection::open_in_memory_with_flags(cfg) {
            Err(e) => println!("open_with_flags -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
            Ok(c) => {
                c.execute_batch("INSTALL httpfs").ok();
                say("  LOAD httpfs", c.execute_batch("LOAD httpfs"));
                println!(
                    "  query, config-only creds = {}",
                    scalar(&c, "SELECT count(*) FROM read_parquet('s3://xdu-probe/big/alice/*.parquet')")
                );
            }
        },
    }
}

/// Claim 7 and the two open items: the marker reads over S3 without a Rust client, but
/// nothing bounds that read; and the AWS credential chain wants the `aws` extension.
#[test]
fn claim_7_marker_read_and_the_open_items() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("INSTALL httpfs").ok();
    c.execute_batch("LOAD httpfs").unwrap();
    c.execute_batch(&secret_sql()).unwrap();

    let marker = "s3://xdu-probe/indexes/scratch/.xdu-complete";
    println!("marker via read_text  = {}", scalar(&c, &format!("SELECT substring(content,1,24) FROM read_text('{marker}')")));
    println!(
        "marker as bound param = {}",
        c.query_row("SELECT substring(content,1,12) FROM read_text(?)", [marker], |r| r.get::<_, String>(0))
            .unwrap_or_else(|e| format!("ERR: {}", e.to_string().lines().next().unwrap_or("")))
    );

    // Open item: nothing caps this. read_blob reports the size only after fetching it,
    // and glob() exposes `file` alone -- so MARKER_READ_LIMIT has no equivalent here.
    println!("read_blob size        = {}", scalar(&c, &format!("SELECT octet_length(content)::BIGINT FROM read_blob('{marker}')")));
    println!(
        "glob() columns        = {}",
        scalar(&c, "SELECT string_agg(column_name,',') FROM (DESCRIBE SELECT * FROM glob('s3://xdu-probe/big/*/*.parquet'))")
    );

    // Open item: the credential chain is a second extension, not a config flag.
    say(
        "CREATE SECRET PROVIDER credential_chain",
        c.execute_batch("CREATE SECRET cc (TYPE s3, PROVIDER credential_chain)"),
    );
}
