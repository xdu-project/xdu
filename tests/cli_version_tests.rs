//! Integration tests for the `-V` / `--version` flag on every user-facing binary.
//!
//! These drive the **real** binaries and assert each one prints the `Cargo.toml`
//! package version and exits 0 for both flag spellings, so the man pages never
//! again promise a flag the binaries reject.

mod common;

use std::process::Command;

use common::binary_path;

/// The package version is the only version source; the test holds no literal.
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every user-facing binary answers both version spellings with the package version.
#[test]
fn test_every_binary_reports_package_version() {
    for binary in ["xdu", "xdu-find", "xdu-view", "xdu-rm", "xdu-cp", "xdu-mv"] {
        for flag in ["--version", "-V"] {
            let output = Command::new(binary_path(binary))
                .arg(flag)
                .output()
                .unwrap_or_else(|e| panic!("failed to spawn {binary} {flag}: {e}"));
            assert!(
                output.status.success(),
                "{binary} {flag} must exit 0: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            assert!(
                stdout.contains(PACKAGE_VERSION),
                "{binary} {flag} must print {PACKAGE_VERSION}, got: {stdout:?}"
            );
        }
    }
}
