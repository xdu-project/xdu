//! Can the bundled DuckDB load `httpfs`, and can that be done without a network
//! or a write to `$HOME`? Establishes claims 1-5 of
//! `research/duckdb-native-s3-reads.md`. Needs no S3 endpoint; claim 1 needs the
//! network once, to populate the extension directory the later claims read from.
//!
//! See `bench/spikes/README.md` for how to run this. It is not part of `cargo test`.

use duckdb::Connection;

fn say(label: &str, r: duckdb::Result<()>) {
    match r {
        Ok(()) => println!("{label} -> OK"),
        Err(e) => println!("{label} -> ERR: {}", e.to_string().lines().next().unwrap_or("")),
    }
}

fn scalar(c: &Connection, sql: &str) -> String {
    c.query_row(sql, [], |r| r.get::<_, String>(0))
        .unwrap_or_else(|e| format!("ERR: {}", e.to_string().lines().next().unwrap_or("")))
}

fn walk(d: &std::path::Path) -> Vec<String> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                v.extend(walk(&p));
            } else {
                v.push(p.display().to_string());
            }
        }
    }
    v
}

/// Claim 1: the statically compiled engine loads the official prebuilt extension.
#[test]
fn claim_1_bundled_engine_loads_prebuilt_httpfs() {
    let c = Connection::open_in_memory().unwrap();
    println!("version  = {}", scalar(&c, "SELECT version()"));
    println!("platform = {}", scalar(&c, "PRAGMA platform"));
    say("INSTALL httpfs", c.execute_batch("INSTALL httpfs"));
    say("LOAD httpfs", c.execute_batch("LOAD httpfs"));
    println!(
        "installed/loaded = {}",
        scalar(
            &c,
            "SELECT coalesce(string_agg(extension_name || '=' || loaded, ', '), '<none>') \
             FROM duckdb_extensions() WHERE installed OR loaded"
        )
    );
    // Explicit key/secret needs no second extension. Contrast with capability.rs,
    // where PROVIDER credential_chain fails for want of the `aws` extension.
    say(
        "CREATE SECRET (TYPE s3)",
        c.execute_batch(
            "CREATE SECRET probe (TYPE s3, KEY_ID 'x', SECRET 'y', \
             ENDPOINT 'localhost:9000', URL_STYLE 'path', USE_SSL false)",
        ),
    );
}

/// Claims 2-5: the air-gap mechanics. Redirect the extension directory, load from a
/// pre-seeded one with no network, load by absolute path, and fail legibly when absent.
#[test]
fn claims_2_to_5_airgap_mechanics() {
    let extdir = std::env::temp_dir().join("xdu-spike-extdir");
    let cold = std::env::temp_dir().join("xdu-spike-coldhome");
    let empty = std::env::temp_dir().join("xdu-spike-noextdir");
    for d in [&extdir, &cold, &empty] {
        let _ = std::fs::remove_dir_all(d);
        std::fs::create_dir_all(d).unwrap();
    }

    // Claim 2 -- installs land where they are told to, not in $HOME.
    let c = Connection::open_in_memory().unwrap();
    say(
        "SET extension_directory",
        c.execute_batch(&format!("SET extension_directory='{}'", extdir.display())),
    );
    say("INSTALL httpfs (redirected)", c.execute_batch("INSTALL httpfs"));
    say("LOAD httpfs (redirected)", c.execute_batch("LOAD httpfs"));
    drop(c);
    for f in walk(&extdir) {
        println!("  seeded: {f}");
    }

    // Claim 3 -- a pre-seeded directory serves a cold HOME with no install and no network.
    unsafe { std::env::set_var("HOME", &cold) };
    let c2 = Connection::open_in_memory().unwrap();
    say(
        "SET extension_directory (cold)",
        c2.execute_batch(&format!("SET extension_directory='{}'", extdir.display())),
    );
    say(
        "SET autoinstall_known_extensions=false",
        c2.execute_batch("SET autoinstall_known_extensions=false"),
    );
    say(
        "SET autoload_known_extensions=false",
        c2.execute_batch("SET autoload_known_extensions=false"),
    );
    say("LOAD httpfs (cold, preseeded)", c2.execute_batch("LOAD httpfs"));
    println!(
        "cold loaded = {}",
        scalar(
            &c2,
            "SELECT coalesce(string_agg(extension_name, ', '), '<none>') \
             FROM duckdb_extensions() WHERE loaded"
        )
    );

    // Claim 4 -- a vendored binary loads by absolute path, which is the shipping story.
    if let Some(p) = walk(&extdir)
        .into_iter()
        .find(|p| p.ends_with("httpfs.duckdb_extension"))
    {
        let c3 = Connection::open_in_memory().unwrap();
        say("LOAD '<abs path>'", c3.execute_batch(&format!("LOAD '{p}'")));
    }

    // Claim 5 -- absent extension with autoinstall off names the path it wanted.
    let c4 = Connection::open_in_memory().unwrap();
    say(
        "SET extension_directory (empty)",
        c4.execute_batch(&format!("SET extension_directory='{}'", empty.display())),
    );
    say(
        "SET autoinstall_known_extensions=false",
        c4.execute_batch("SET autoinstall_known_extensions=false"),
    );
    say("LOAD httpfs (absent)", c4.execute_batch("LOAD httpfs"));

    // Claim 3's other half: nothing was written to the redirected HOME.
    let leaked = walk(&cold);
    println!("cold HOME entries = {} {:?}", leaked.len(), leaked);
}
