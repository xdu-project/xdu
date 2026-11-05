//! Every DuckDB connection xdu opens, and every extension it loads.
//!
//! The bundled engine is compiled with autoinstall and autoload switched on, so a query that
//! touches a capability the binary does not link reaches for the network and writes into
//! `$HOME/.duckdb/extensions/`. On an air-gapped login node that is a failure, and on a
//! connected one it is a write nobody asked for. Connections opened here refuse both, which
//! makes the offline guarantee a refusal rather than the absence of a trigger.
//!
//! Extensions xdu needs are shipped beside the binaries, at `<prefix>/lib/xdu/extensions/`,
//! and load only by absolute path. An absent file is an error naming that path; nothing here
//! downloads. `clippy.toml` forbids opening a `duckdb::Connection` anywhere else, so no reader
//! can open one that skips the refusal.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use duckdb::{Config, Connection, params_from_iter};

use crate::s3::S3Settings;

/// The extensions a release ships, by DuckDB name.
///
/// The one statement of the set: the loader refuses any other name, and the release scripts
/// read it through the probe's manifest. Shipping another extension is one more entry here.
pub const PINNED_EXTENSIONS: &[&str] = &["httpfs"];

/// Environment variable that overrides the extension directory, for hosts that install the
/// extensions somewhere other than beside the binaries.
pub const EXTENSION_DIR_ENV: &str = "XDU_EXTENSION_DIR";

/// Where the extensions sit relative to the installation prefix, the directory above `bin/`.
pub const EXTENSION_SUBDIR: &str = "lib/xdu/extensions";

/// Open an in-memory DuckDB connection with autoinstall and autoload off.
///
/// `enable_autoload_extension(false)` sets both `autoinstall_known_extensions` and
/// `autoload_known_extensions`, and the setting applies at open, before any query can run.
#[allow(clippy::disallowed_methods)]
pub fn open_connection() -> duckdb::Result<Connection> {
    let config = Config::default().enable_autoload_extension(false)?;
    Connection::open_in_memory_with_flags(config)
}

/// An override counts only when it is set and non-empty: `XDU_EXTENSION_DIR=` in a wrapper
/// script means "unset", not "the current directory".
fn effective_override(override_dir: Option<&OsStr>) -> Option<&OsStr> {
    override_dir.filter(|dir| !dir.is_empty())
}

/// The extension directory for a given override and executable path.
///
/// A non-empty override wins and is returned as given. It is never canonicalized:
/// canonicalizing a directory that does not exist yields a bare `os error 2`, and the operator
/// would lose the path the loader looked in. Otherwise the result is the executable's
/// grandparent joined with `EXTENSION_SUBDIR`. Joining `exe/../..` instead would fail with
/// `ENOTDIR`, since the executable is a file.
pub fn extension_dir_from(override_dir: Option<&OsStr>, exe: &Path) -> Result<PathBuf> {
    if let Some(dir) = effective_override(override_dir) {
        return Ok(PathBuf::from(dir));
    }
    let prefix = exe.parent().and_then(Path::parent).ok_or_else(|| {
        anyhow!(
            "cannot derive an installation prefix from executable {}; set {} to the directory \
             holding the DuckDB extensions",
            exe.display(),
            EXTENSION_DIR_ENV
        )
    })?;
    Ok(prefix.join(EXTENSION_SUBDIR))
}

/// The extension directory in effect for this process.
///
/// Only the executable path is canonicalized, so a symlinked binary resolves to the tree it
/// was installed into. The override is read first, so a host that sets it does not depend on
/// `current_exe()` succeeding at all.
pub fn extension_dir() -> Result<PathBuf> {
    let override_dir = env::var_os(EXTENSION_DIR_ENV);
    if effective_override(override_dir.as_deref()).is_some() {
        return extension_dir_from(override_dir.as_deref(), Path::new(""));
    }
    // A binary replaced or deleted while running fails here, and the message says which
    // lookup it was rather than surfacing a bare I/O error.
    let exe = env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .with_context(|| {
            format!(
                "cannot resolve the running executable to find its DuckDB extensions; set {} \
                 to the directory holding them",
                EXTENSION_DIR_ENV
            )
        })?;
    extension_dir_from(None, &exe)
}

/// Load a pinned extension from `dir` by absolute path, returning the path loaded.
///
/// The file must already be there. An absent one is an error naming the full path it looked
/// for, before any SQL runs, and nothing is fetched. The path reaches SQL as a string literal
/// and can come from an environment variable, so a path containing `'` or not valid UTF-8 is
/// refused outright rather than escaped.
pub fn load_extension_from(conn: &Connection, dir: &Path, name: &str) -> Result<PathBuf> {
    if !PINNED_EXTENSIONS.contains(&name) {
        bail!(
            "DuckDB extension '{}' is not one xdu ships (shipped: {})",
            name,
            PINNED_EXTENSIONS.join(", ")
        );
    }

    let file = dir.join(format!("{name}.duckdb_extension"));
    let is_file = fs::metadata(&file).map(|m| m.is_file()).unwrap_or(false);
    if !is_file {
        bail!(
            "DuckDB extension '{}' not found at {}; xdu does not download extensions, and a \
             `cargo install` build ships none. Copy {}/ from the xdu release tarball for this \
             version and platform into the installation prefix, or set {} to the directory \
             that holds the file",
            name,
            file.display(),
            EXTENSION_SUBDIR,
            EXTENSION_DIR_ENV
        );
    }

    let file = std::path::absolute(&file)
        .with_context(|| format!("cannot make extension path absolute: {}", file.display()))?;
    let Some(literal) = file.to_str() else {
        bail!(
            "refusing to load DuckDB extension from a path that is not valid UTF-8: {}",
            file.display()
        );
    };
    if literal.contains('\'') {
        bail!(
            "refusing to load DuckDB extension from a path containing a single quote: {}",
            literal
        );
    }

    conn.execute_batch(&format!("LOAD '{literal}'"))
        .with_context(|| format!("failed to load DuckDB extension {}", literal))?;
    Ok(file)
}

/// Load a pinned extension from the extension directory in effect.
pub fn load_extension(conn: &Connection, name: &str) -> Result<PathBuf> {
    let dir = extension_dir()?;
    load_extension_from(conn, &dir, name)
}

/// Open a connection that reads `s3://` paths with the given credentials.
///
/// `open_connection`, then the pinned `httpfs` from the extension directory in effect, then
/// one prepared `CREATE SECRET` whose every value is a bound parameter. A local index never
/// comes through here, so a local query loads no extension at all.
pub fn open_s3_connection(settings: &S3Settings) -> Result<Connection> {
    let dir = extension_dir()?;
    open_s3_connection_from(settings, &dir)
}

/// `open_s3_connection` with the extension directory given.
///
/// The secret goes in after the `LOAD`: a `Config` key naming an `s3_*` setting is refused at
/// open while autoload is off, and turning autoload on would reopen the download path that
/// `open_connection` closes. Held as a secret, the key shows as `redacted` in
/// `duckdb_secrets()` and appears in no `duckdb_settings()` row, where a `SET` would print it.
pub fn open_s3_connection_from(settings: &S3Settings, dir: &Path) -> Result<Connection> {
    let conn = open_connection()?;
    load_extension_from(&conn, dir, "httpfs")?;
    let (sql, params) = settings.duckdb_secret_statement();
    conn.execute(&sql, params_from_iter(params))
        .context("failed to hand the S3 credentials to DuckDB")?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::ffi::OsStrExt;

    use tempfile::TempDir;

    fn entries(dir: &Path) -> usize {
        fs::read_dir(dir).unwrap().count()
    }

    /// The wording DuckDB appends to a missing-extension error only when autoload is off.
    const AUTOLOAD_OFF_HINT: &str = "SET autoload_known_extensions=1";

    #[test]
    fn json_cast_is_refused_without_installing() {
        // `json` is in the amalgamation but not linked, so the cast is a purely local autoload
        // trigger. With autoload on the engine tries to install it into the extension
        // directory; with it off the cast fails and the directory stays empty.
        let ext_dir = TempDir::new().unwrap();
        let ext_path = ext_dir.path().to_str().unwrap();
        assert!(!ext_path.contains('\''));
        let conn = open_connection().unwrap();
        conn.execute_batch(&format!("SET extension_directory = '{ext_path}'"))
            .unwrap();

        let err = conn
            .query_row("SELECT '{}'::JSON", [], |row| row.get::<_, String>(0))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(AUTOLOAD_OFF_HINT),
            "expected the autoload-disabled hint, got: {err}"
        );
        assert_eq!(
            entries(ext_dir.path()),
            0,
            "extension directory was written"
        );
    }

    #[test]
    fn connections_open_with_autoinstall_and_autoload_off() {
        let conn = open_connection().unwrap();
        let (autoinstall, autoload): (bool, bool) = conn
            .query_row(
                "SELECT current_setting('autoinstall_known_extensions')::BOOLEAN, \
                        current_setting('autoload_known_extensions')::BOOLEAN",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(!autoinstall, "autoinstall_known_extensions is on");
        assert!(!autoload, "autoload_known_extensions is on");
    }

    fn assert_names_absent_file(dir: &Path) {
        let conn = open_connection().unwrap();
        let err = format!(
            "{:#}",
            load_extension_from(&conn, dir, "httpfs").unwrap_err()
        );
        let expected = dir.join("httpfs.duckdb_extension");
        assert!(
            err.contains(expected.to_str().unwrap()),
            "error does not name {}: {err}",
            expected.display()
        );
        assert!(
            err.contains(EXTENSION_DIR_ENV),
            "error does not name the override: {err}"
        );
        assert!(
            err.contains("does not download"),
            "error does not say it will not fetch: {err}"
        );
        assert!(
            err.contains("release tarball"),
            "error does not name the release-tarball remedy: {err}"
        );
    }

    #[test]
    fn absent_extension_fails_naming_the_path() {
        let dir = TempDir::new().unwrap();
        assert_names_absent_file(dir.path());
        assert_eq!(
            entries(dir.path()),
            0,
            "the extension directory was written"
        );
    }

    #[test]
    fn missing_override_directory_fails_naming_the_path_as_given() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("not-there");
        assert_names_absent_file(&dir);
        assert!(!dir.exists(), "the missing directory was created");
    }

    #[test]
    fn unpinned_extension_is_refused() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("json.duckdb_extension"),
            b"not an extension",
        )
        .unwrap();
        let conn = open_connection().unwrap();
        let err = load_extension_from(&conn, dir.path(), "json")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not one xdu ships"), "unexpected error: {err}");
    }

    #[test]
    fn quoted_directory_is_refused_before_any_sql() {
        // A file is present, so only the quote check stands between this path and `LOAD`. Had
        // the statement run, the error would be DuckDB's, about a file that is not an extension.
        let root = TempDir::new().unwrap();
        let dir = root.path().join("it's");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("httpfs.duckdb_extension"), b"not an extension").unwrap();
        let conn = open_connection().unwrap();
        let err = format!(
            "{:#}",
            load_extension_from(&conn, &dir, "httpfs").unwrap_err()
        );
        assert!(err.contains("single quote"), "unexpected error: {err}");
        assert!(!err.contains("failed to load"), "LOAD ran: {err}");
    }

    #[test]
    fn non_utf8_directory_is_refused_before_any_sql() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join(OsStr::from_bytes(b"ext-\xff"));
        if fs::create_dir(&dir).is_err() {
            // APFS rejects names that are not valid UTF-8; the Linux CI leg runs this case.
            eprintln!("skipping: filesystem rejects a non-UTF-8 directory name");
            return;
        }
        fs::write(dir.join("httpfs.duckdb_extension"), b"not an extension").unwrap();
        let conn = open_connection().unwrap();
        let err = format!(
            "{:#}",
            load_extension_from(&conn, &dir, "httpfs").unwrap_err()
        );
        assert!(err.contains("not valid UTF-8"), "unexpected error: {err}");
    }

    #[test]
    fn override_wins_unchanged() {
        let exe = Path::new("/opt/xdu/bin/xdu-find");
        for given in ["/srv/duckdb-ext", "relative/ext", "/does/not/exist/../ext"] {
            let dir = extension_dir_from(Some(OsStr::new(given)), exe).unwrap();
            assert_eq!(dir, PathBuf::from(given));
        }
    }

    #[test]
    fn empty_override_is_ignored() {
        let exe = Path::new("/opt/xdu/bin/xdu-find");
        let dir = extension_dir_from(Some(OsStr::new("")), exe).unwrap();
        assert_eq!(dir, PathBuf::from("/opt/xdu/lib/xdu/extensions"));
    }

    #[test]
    fn default_is_beside_the_executable_prefix() {
        let exe = Path::new("/opt/xdu/bin/xdu-find");
        let dir = extension_dir_from(None, exe).unwrap();
        assert_eq!(dir, PathBuf::from("/opt/xdu/lib/xdu/extensions"));
    }

    #[test]
    fn executable_without_a_prefix_is_an_error() {
        let err = extension_dir_from(None, Path::new("/xdu-find"))
            .unwrap_err()
            .to_string();
        assert!(err.contains(EXTENSION_DIR_ENV), "unexpected error: {err}");
    }
}
