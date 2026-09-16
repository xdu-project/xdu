//! Runs exactly one query per process, so every HTTP request the logging proxy records
//! is attributable to it. That one-query-per-invocation discipline is what makes the
//! byte accounting in `research/duckdb-native-s3-reads.md` trustworthy; a long-lived
//! connection would blur caching across queries and inflate the cheap ones.
//!
//! Driven by `measure.py`, which truncates the proxy log, runs this, and reads the log
//! back. Point `EP` at the proxy (19001) to be measured, or the endpoint (19000) to not.
//!
//! See `bench/spikes/README.md`. Not part of `cargo test`.

use duckdb::Connection;

#[test]
fn one_query() {
    let Ok(sql) = std::env::var("SQL") else {
        println!("set SQL=<query>; nothing run");
        return;
    };
    let ep = std::env::var("EP").unwrap_or_else(|_| "127.0.0.1:19001".into());

    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("INSTALL httpfs").ok();
    c.execute_batch("LOAD httpfs").unwrap();
    c.execute_batch(&format!(
        "CREATE SECRET x (TYPE s3, KEY_ID 'probekey', SECRET 'probesecret', REGION 'us-east-1', \
         ENDPOINT '{ep}', URL_STYLE 'path', USE_SSL false)"
    ))
    .unwrap();

    let t = std::time::Instant::now();
    let v = c
        .query_row(&sql, [], |r| {
            r.get::<_, String>(0)
                .or_else(|_| r.get::<_, i64>(0).map(|x| x.to_string()))
        })
        .unwrap_or_else(|e| format!("ERR: {}", e.to_string().lines().next().unwrap_or("")));

    // measure.py parses exactly this line.
    println!("RESULT={v} MS={}", t.elapsed().as_millis());
}
