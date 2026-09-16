//! Builds the S3 fixtures the cost tables in `research/duckdb-native-s3-reads.md` were
//! measured against. Writing them through `COPY ... TO 's3://…'` is itself claim 12.
//!
//! Selected by the `FIXTURE` env var so one file serves three shapes:
//!   FIXTURE=big      four partitions x 1M rows      -> s3://xdu-probe/big/
//!   FIXTURE=hive     120 two-level prefixes, x2     -> s3://xdu-hive/{two,mixed}/
//!   FIXTURE=cluster  row-group x sort-order matrix  -> s3://xdu-hive/c_*/
//!
//! Writes go direct to the endpoint, never through the logging proxy -- the proxy is for
//! attributing reads. See `bench/spikes/README.md`. Not part of `cargo test`.

use duckdb::Connection;

fn conn(ep: &str) -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("INSTALL httpfs").ok();
    c.execute_batch("LOAD httpfs").unwrap();
    c.execute_batch(&format!(
        "CREATE SECRET x (TYPE s3, KEY_ID 'probekey', SECRET 'probesecret', REGION 'us-east-1', \
         ENDPOINT '{ep}', URL_STYLE 'path', USE_SSL false)"
    ))
    .unwrap();
    c
}

#[test]
fn build() {
    let ep = std::env::var("EP").unwrap_or_else(|_| "127.0.0.1:19000".into());
    let which = match std::env::var("FIXTURE") {
        Ok(v) => v,
        Err(_) => {
            println!("set FIXTURE=big|hive|cluster; nothing built");
            return;
        }
    };
    let c = conn(&ep);
    match which.as_str() {
        // 80.1 MiB across four partitions -- the index the read cost table uses.
        "big" => {
            for (i, p) in ["alice", "bob", "carol", "__root__"].iter().enumerate() {
                let lo = (i as i64) * 10_000_000;
                let sql = format!(
                    "COPY (SELECT '/data/{p}/file' || x AS path, {lo} + x AS size, \
                     1000::BIGINT AS uid, 1000::BIGINT AS gid, 33188::BIGINT AS \"mode\", \
                     1700000000 + x AS atime, 1700000000 + x AS mtime, 1700000000 + x AS ctime \
                     FROM range(1000000) t(x)) \
                     TO 's3://xdu-probe/big/{p}/000000.parquet' (FORMAT parquet, COMPRESSION snappy)"
                );
                report(&c, &sql, p);
            }
        }
        // 120 prefixes, written twice: uniformly hive-named, and with a bare top level,
        // which is how the mixed-path shape was shown to work.
        "hive" => {
            for i in 0..120 {
                let y = 2020 + (i % 5);
                let body = format!(
                    "SELECT '/d/p{i}/f' || x AS path, ({i}*1000 + x)::BIGINT AS size, \
                     1000::BIGINT AS uid, 1000::BIGINT AS gid, 33188::BIGINT AS \"mode\", \
                     {y}0000000::BIGINT AS atime, 0::BIGINT AS mtime, 0::BIGINT AS ctime \
                     FROM range(5000) t(x)"
                );
                let opts = "(FORMAT parquet, COMPRESSION snappy)";
                report(&c, &format!("COPY ({body}) TO 's3://xdu-hive/two/partition=p{i}/ayear={y}/000000.parquet' {opts}"), "two");
                report(&c, &format!("COPY ({body}) TO 's3://xdu-hive/mixed/p{i}/ayear={y}/000000.parquet' {opts}"), "mixed");
            }
        }
        // The clustering matrix. `atime` is a permutation of row order, so walk order is
        // uncorrelated with it -- which is the situation a real crawl produces.
        "cluster" => {
            for p in 0..10 {
                let body = format!(
                    "SELECT '/d/p{p}/f'||x AS path, (x*7919%100000)::BIGINT AS size, \
                     1000::BIGINT AS uid, 1000::BIGINT AS gid, 33188::BIGINT AS \"mode\", \
                     (1600000000 + (x*7919%100000)*300)::BIGINT AS atime, 0::BIGINT AS mtime, \
                     0::BIGINT AS ctime FROM range(100000) t(x)"
                );
                // ROW_GROUP_SIZE 1000000 reproduces what xdu writes today: one group per
                // 100k-row chunk, because parquet's default max (1048576) is never reached.
                for (name, order, rg) in [
                    ("c_walk_1rg", "", 1_000_000),
                    ("c_sort_1rg", " ORDER BY atime", 1_000_000),
                    ("c_walk_10rg", "", 10_000),
                    ("c_sort_10rg", " ORDER BY atime", 10_000),
                ] {
                    report(&c, &format!(
                        "COPY ({body}{order}) TO 's3://xdu-hive/{name}/p{p}/000000.parquet' \
                         (FORMAT parquet, COMPRESSION snappy, ROW_GROUP_SIZE {rg})"
                    ), name);
                }
            }
        }
        other => println!("unknown FIXTURE={other}"),
    }
}

fn report(c: &Connection, sql: &str, label: &str) {
    match c.execute_batch(sql) {
        Ok(()) => println!("COPY TO s3 {label} -> OK"),
        Err(e) => println!("COPY TO s3 {label} -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
    }
}
