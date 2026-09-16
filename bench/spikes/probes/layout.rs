//! Layout semantics, all local -- no S3 endpoint needed. Establishes the
//! "not Hive-partitioned" finding in `research/duckdb-native-s3-reads.md` and the four
//! repack traps recorded in `issues/xdu-repack.md`.
//!
//! See `bench/spikes/README.md`. It is not part of `cargo test`.

use duckdb::Connection;

/// `mode` is a reserved word, so the eight-column projection quotes it. Forgetting that
/// is a parser error, not a wrong answer, but it costs a run.
fn eight_columns(prefix: &str, lo: i64, rows: i64) -> String {
    format!(
        "SELECT '{prefix}/f' || x AS path, {lo} + x AS size, 1000::BIGINT AS uid, \
         1000::BIGINT AS gid, 33188::BIGINT AS \"mode\", 1700000000::BIGINT AS atime, \
         1700000000::BIGINT AS mtime, 1700000000::BIGINT AS ctime FROM range({rows}) t(x)"
    )
}

fn rows(c: &Connection, sql: &str) -> String {
    let mut stmt = match c.prepare(sql) {
        Ok(s) => s,
        Err(e) => return format!("ERR: {}", e.to_string().lines().next().unwrap_or("")),
    };
    let mut out = String::new();
    let mut r = match stmt.query([]) {
        Ok(r) => r,
        Err(e) => return format!("ERR: {}", e.to_string().lines().next().unwrap_or("")),
    };
    // column_count() is only valid once the statement has executed.
    let n = r.as_ref().map(|s| s.column_count()).unwrap_or(0);
    while let Ok(Some(row)) = r.next() {
        for i in 0..n {
            let v = row
                .get::<_, String>(i)
                .or_else(|_| row.get::<_, i64>(i).map(|x| x.to_string()))
                .unwrap_or_else(|_| "?".into());
            out.push_str(&v);
            out.push('|');
        }
        out.push('\n');
    }
    out
}

/// The layout is not Hive-partitioned: bare directory names yield no partition column,
/// while `key=value` names do. This is the whole basis of the prose correction.
#[test]
fn bare_directories_are_not_hive_partitions() {
    let root = std::env::temp_dir().join("xdu-spike-hive");
    let _ = std::fs::remove_dir_all(&root);
    let c = Connection::open_in_memory().unwrap();

    for (i, p) in ["alice", "bob", "__root__"].iter().enumerate() {
        std::fs::create_dir_all(root.join("plain").join(p)).unwrap();
        std::fs::create_dir_all(root.join("hive").join(format!("partition={p}"))).unwrap();
        let body = eight_columns(&format!("/{p}"), (i as i64) * 1_000_000, 200_000);
        let opts = "(FORMAT parquet, COMPRESSION snappy, ROW_GROUP_SIZE 20000)";
        c.execute_batch(&format!("COPY ({body}) TO '{}/plain/{p}/000000.parquet' {opts}", root.display())).unwrap();
        c.execute_batch(&format!("COPY ({body}) TO '{}/hive/partition={p}/000000.parquet' {opts}", root.display())).unwrap();
    }

    println!("-- xdu's actual layout, hive_partitioning=true --");
    println!("first row (8 values = no partition column was derived):");
    println!("{}", rows(&c, &format!("SELECT * FROM read_parquet('{}/plain/*/*.parquet', hive_partitioning=true) LIMIT 1", root.display())));

    println!("-- the same rows under partition=<name>/ --");
    println!("WHERE partition='bob' -> {}", rows(&c, &format!(
        "SELECT partition, count(*) FROM read_parquet('{}/hive/*/*.parquet', hive_partitioning=true) \
         WHERE partition='bob' GROUP BY 1", root.display())));

    println!("-- the handle xdu-view actually uses, which survives an s3:// path --");
    println!("{}", rows(&c, &format!(
        "SELECT DISTINCT regexp_extract(filename, '.*/([^/]+)/[^/]+\\.parquet$', 1) \
         FROM read_parquet('{}/plain/*/*.parquet', filename=true)", root.display())));
}

/// `COPY ... PARTITION_BY` is the whole of a repack, and carries four traps.
#[test]
fn repack_via_partition_by_and_its_traps() {
    let root = std::env::temp_dir().join("xdu-spike-repack");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let c = Connection::open_in_memory().unwrap();

    for (i, p) in ["alice", "bob", "__root__"].iter().enumerate() {
        std::fs::create_dir_all(root.join("src").join(p)).unwrap();
        let body = format!(
            "SELECT '/d/{p}/f'||x AS path, (x*13%9000)::BIGINT AS size, (1000+{i})::BIGINT AS uid, \
             1000::BIGINT AS gid, 33188::BIGINT AS \"mode\", (1600000000 + (x%4)*31536000)::BIGINT AS atime, \
             0::BIGINT AS mtime, 0::BIGINT AS ctime FROM range(20000) t(x)"
        );
        c.execute_batch(&format!("COPY ({body}) TO '{}/src/{p}/000000.parquet' (FORMAT parquet, COMPRESSION snappy)", root.display())).unwrap();
    }
    let src = format!("{}/src/*/*.parquet", root.display());
    println!("source rows = {}", rows(&c, &format!("SELECT count(*) FROM read_parquet('{src}')")));

    // Trap 2 is live in this statement: SELECT * with filename=true carries `filename`
    // into the output, which a real repack must project away.
    let sql = format!(
        "COPY (SELECT *, regexp_extract(filename, '.*/([^/]+)/[^/]+\\.parquet$', 1) AS part, \
              ('y' || (1970 + atime//31536000)::VARCHAR) AS ayear \
         FROM read_parquet('{src}', filename=true)) \
         TO '{}/dst' (FORMAT parquet, COMPRESSION snappy, PARTITION_BY (part, ayear), OVERWRITE_OR_IGNORE)",
        root.display()
    );
    match c.execute_batch(&sql) {
        Ok(()) => println!("PARTITION_BY repack -> OK"),
        Err(e) => println!("PARTITION_BY -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
    }

    let mut found = vec![];
    fn walk(d: &std::path::Path, out: &mut Vec<String>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out) } else { out.push(p.display().to_string()) }
            }
        }
    }
    walk(&root.join("dst"), &mut found);
    found.sort();
    // Trap 3: the writer names these data_0.parquet, not the zero-padded NNNNNN form
    // that stale-chunk pruning keys off.
    for f in found.iter().take(4) {
        println!("  {}", f.replace(&format!("{}/dst/", root.display()), ""));
    }
    println!("  ... {} files", found.len());

    // Trap 1: partition keys are STRIPPED from the written files -- `part` and `ayear`
    // are absent below. Partitioning on a schema column would delete it from the index.
    println!("columns written = {}", rows(&c, &format!(
        "SELECT string_agg(name, ',') FROM (SELECT * FROM parquet_schema('{}') \
         WHERE num_children IS NULL OR num_children = 0)", found.first().cloned().unwrap_or_default())));
    println!("rows back (hive on) = {}", rows(&c, &format!(
        "SELECT count(*) FROM read_parquet('{}/dst/*/*/*.parquet', hive_partitioning=true)", root.display())));
    println!("filter on derived key = {}", rows(&c, &format!(
        "SELECT count(*) FROM read_parquet('{}/dst/*/*/*.parquet', hive_partitioning=true) WHERE ayear='y2020'", root.display())));
    println!("uid still selectable = {}", rows(&c, &format!(
        "SELECT count(DISTINCT uid)::BIGINT FROM read_parquet('{}/dst/*/*/*.parquet')", root.display())));
}
