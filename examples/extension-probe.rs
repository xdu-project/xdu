//! Release-time probe for the DuckDB extensions a tarball ships.
//!
//! An example rather than a binary, so `cargo install` never installs it. The release legs
//! build it in the same `cargo build` invocation as the tarball binaries, so it carries the
//! same engine, toolchain and link flags, and it reaches the engine only through
//! `xdu::engine`, the path a shipped tool would take.
//!
//! `manifest` prints what this engine is, so the packaging scripts never hard-code a version,
//! a platform or an extension name:
//!
//! ```text
//! engine=v1.4.4
//! platform=osx_arm64
//! source_id=<engine source id>
//! extension=httpfs
//! ```
//!
//! `load` resolves the extension directory as an installed tool would, loads every pinned
//! extension from it, prints `loaded=<name> <path>` for each, and exits non-zero on any miss.
//! Everything runs on one connection: each `open_connection` is a separate in-memory
//! database, so a check made on a second connection would never see what the first loaded.

use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail, ensure};
use duckdb::Connection;

use xdu::engine::{self, PINNED_EXTENSIONS};

/// Platforms a release leg builds. Anything else means the engine reports a platform the
/// extension repository is not being asked for, and the fetch would 404 or fetch the wrong file.
const PLATFORMS: &[&str] = &["linux_amd64", "linux_arm64", "osx_amd64", "osx_arm64"];

/// The wording DuckDB appends to a missing-extension error only when autoload is off.
const AUTOLOAD_OFF_HINT: &str = "SET autoload_known_extensions=1";

/// A port nothing listens on, so a read through `httpfs` fails inside the extension.
const REFUSED_URL: &str = "http://127.0.0.1:9/x.parquet";

fn main() -> ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("manifest") => manifest(),
        Some("load") => load(),
        _ => Err(anyhow!("usage: extension-probe manifest|load")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("extension-probe: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// `vX.Y.Z` with numeric parts, the form the extension repository's URLs use.
fn is_engine_version(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('v') else {
        return false;
    };
    let parts: Vec<&str> = rest.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn manifest() -> Result<()> {
    let conn = engine::open_connection()?;
    let (engine_version, source_id): (String, String) = conn
        .query_row(
            "SELECT library_version, source_id FROM pragma_version()",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .context("reading the engine version")?;
    let platform: String = conn
        .query_row("PRAGMA platform", [], |row| row.get(0))
        .context("reading the engine platform")?;

    ensure!(
        is_engine_version(&engine_version),
        "engine version {engine_version:?} is not shaped vX.Y.Z"
    );
    ensure!(
        PLATFORMS.contains(&platform.as_str()),
        "platform {platform:?} is not one a release leg builds"
    );
    ensure!(!source_id.trim().is_empty(), "engine source_id is empty");
    ensure!(
        !PINNED_EXTENSIONS.is_empty(),
        "the pinned extension set is empty"
    );

    println!("engine={engine_version}");
    println!("platform={platform}");
    println!("source_id={source_id}");
    for name in PINNED_EXTENSIONS {
        println!("extension={name}");
    }
    Ok(())
}

fn load() -> Result<()> {
    let conn = engine::open_connection()?;

    let (autoinstall, autoload): (bool, bool) = conn.query_row(
        "SELECT current_setting('autoinstall_known_extensions')::BOOLEAN, \
                current_setting('autoload_known_extensions')::BOOLEAN",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    ensure!(
        !autoinstall && !autoload,
        "connection opened with autoinstall={autoinstall} autoload={autoload}"
    );

    // `json` is in the amalgamation but not linked, so this cast is a local autoload trigger.
    // The hint is what tells a refusal apart from an install that failed for want of network.
    match conn.query_row("SELECT '{}'::JSON", [], |row| row.get::<_, String>(0)) {
        Ok(_) => bail!("a JSON cast succeeded, so an unlinked extension was autoloaded"),
        Err(error) => {
            let message = error.to_string();
            ensure!(
                message.contains(AUTOLOAD_OFF_HINT),
                "the JSON cast failed without the autoload-disabled hint: {message}"
            );
        }
    }

    let dir = engine::extension_dir()?;
    for name in PINNED_EXTENSIONS {
        let loaded = engine::load_extension(&conn, name)?;
        ensure!(
            is_loaded(&conn, name)?,
            "{name} loaded from {} but duckdb_extensions() does not report it loaded",
            loaded.display()
        );
        ensure!(
            loaded.starts_with(&dir),
            "{name} loaded from {}, outside {}",
            loaded.display(),
            dir.display()
        );
        println!("loaded={name} {}", loaded.display());
    }

    cross_extension_boundary(&conn)
}

fn is_loaded(conn: &Connection, name: &str) -> Result<bool> {
    let loaded = conn
        .query_row(
            "SELECT loaded FROM duckdb_extensions() WHERE extension_name = ?",
            [name],
            |row| row.get(0),
        )
        .with_context(|| format!("{name} is missing from duckdb_extensions()"))?;
    Ok(loaded)
}

/// A query that fails inside `httpfs` and must come back as a clean SQL error.
///
/// A bare `LOAD` succeeding does not show the engine and the extension agree on the C++
/// runtime: on `linux_amd64` the executable links `libstdc++` statically while the extension
/// binds the shared one, and exceptions and objects cross between them here. An abort fails
/// the leg; so does any wording that means the request never reached the extension.
fn cross_extension_boundary(conn: &Connection) -> Result<()> {
    let sql = format!("SELECT count(*) FROM read_parquet('{REFUSED_URL}')");
    let message = match conn.query_row(&sql, [], |row| row.get::<_, i64>(0)) {
        Ok(rows) => bail!("{REFUSED_URL} answered with {rows} rows; nothing should listen there"),
        Err(error) => error.to_string(),
    };
    let lowered = message.to_lowercase();
    for wording in ["autoload", "install", "requires the extension"] {
        ensure!(
            !lowered.contains(wording),
            "the read never reached httpfs ({wording:?} in the error): {message}"
        );
    }
    ensure!(
        message.contains(REFUSED_URL) && message.contains("HTTP"),
        "the read failed, but not with httpfs's connection error for {REFUSED_URL}: {message}"
    );
    println!("boundary=ok {}", message.lines().next().unwrap_or_default());
    Ok(())
}
