# 02 — Adversarial review of the R1/R6 gate design

> Plan-step adversarial review of PLAN §2.3–§2.6 and §6, 2026-11-04. Checked against the
> `duckdb`/`libduckdb-sys` 1.4.4 sources and the four official `httpfs` v1.4.4 binaries. Nothing was
> built or run in CI. "False green" means a gate passes while the thing it guards is broken.

## Findings

1. **High: an empty or misparsed manifest makes every gate pass vacuously.** If `PROBE manifest`
   aborts (`panic = "abort"` in the release profile) or its format drifts, a loop like
   `for e in $(PROBE manifest | sed -n 's/^extension=//p')` gets zero entries. `set -e` does not
   fire on a command substitution in a `for` list or on a non-final pipeline stage. `fetch.sh`
   fetches nothing, check step 1 expects nothing, the probe's `load` loop runs zero times, and
   step 4's `grep -a -F ""` matches every file. The leg ships no extension and stays green. On the
   Apple legs, bsdtar can also add `._httpfs.duckdb_extension` AppleDouble members, and a glob
   count would include them. **Fix:** write the manifest to a file and check the probe's exit
   status explicitly. Require at least one `extension=` line, and check each field's shape:
   `engine` matches `^v[0-9]+\.[0-9]+\.[0-9]+$`, `platform` is non-empty, `source_id` matches
   `^[0-9a-f]{8,}$`. Step 1 asserts that the `lib/xdu/extensions/` member set equals the manifest
   set exactly. Assemble with `COPYFILE_DISABLE=1`. The probe refuses an empty `PINNED_EXTENSIONS`
   and prints one `loaded=<name> <path>` line per entry. `check.sh` matches each line against
   `<scratch, pwd -P>/lib/xdu/extensions/<name>.duckdb_extension`.

2. **High: the boundary query can pass without crossing the engine/extension boundary.** In the
   crate, `open_connection()` → `open_in_memory_with_flags(":memory:")` creates a new
   `DatabaseInstance` on every call. If `load` loads `httpfs` on one connection and runs the
   `read_parquet('http://127.0.0.1:…')` query on a second one, DuckDB answers with a clean
   "requires the extension httpfs … autoload" error. That counts as a "clean SQL error", so the
   check passes, and nothing was thrown inside the extension and caught by the engine. That
   exception crossing is the one thing the linux_amd64 libstdc++ mitigation exists to exercise.
   **Fix:** run the query on the connection that loaded the extension. Require the error to be
   `httpfs`'s connection failure, naming the loopback URL as an IO/HTTP error, and reject any
   message containing `autoload`, `INSTALL httpfs`, or `requires the extension`.

3. **Medium: neither "autoinstall disabled" (R6) nor "by refusal" (R5) is observed on the release
   artifacts.** Suppose `open_connection` regresses to a plain `Connection::open_in_memory()`, or
   a merge brings that call back into a reader. Step 2 still loads, because `LOAD '<path>'`
   ignores both autoload settings. Step 5 still prints `3`, because `parquet` is static and nothing
   triggers autoload. Step 6's HOME stays empty. Steps 2, 5 and 6 all pass. **Fix:** on the
   connection it loads on, `load` asserts that
   `current_setting('autoinstall_known_extensions')` and `…autoload…` are both false. It also
   runs `'{}'::JSON` and expects the autoload-off hint, `SET autoload_known_extensions=1`, so the
   cold-HOME result reflects a refusal and not just the absence of a trigger. Add a `clippy.toml`
   `disallowed-methods` rule for `duckdb::Connection::open_in_memory`, `open` and
   `open_with_flags`, allowed only inside `src/engine.rs`, so no reader can bypass it. Step 5
   should also run `xdu-rm --dry-run`, as `offline_tests.rs` does.

4. **Medium: the environment can leak into `check.sh`.** If a workflow step exports
   `XDU_EXTENSION_DIR` (job `env:` or `GITHUB_ENV`) pointing at `staging/`, step 2 loads the
   staging copy instead of the extracted one. The `httpfs` binaries contain `http_proxy`/`no_proxy`
   (checked with `strings`), so a runner proxy changes which error the boundary query returns. The
   manylinux images are build images, and their `LD_LIBRARY_PATH` can supply a library that a clean
   RHEL8 host lacks, which weakens the claim-13 observation. If `$cold` is empty or unset,
   `HOME=""` makes DuckDB expand `~` to `/.duckdb`, outside the directory step 6 inspects. **Fix:**
   run every probe and reader invocation under `env -i PATH=/usr/bin:/bin HOME="$cold"`, adding
   `XDU_EXTENSION_DIR` only for the negative control. Assert that `$cold` is a non-empty, existing
   directory before using it.

5. **Medium: canonicalization can break R4's message and redden the macOS negative control.** If
   `extension_dir()` canonicalizes the override or the joined directory, three things go wrong.
   (a) A missing directory, which every `cargo install` build has and which an override typo
   produces, gives a bare `No such file or directory (os error 2)` with no path. That breaks R4,
   and no gate catches it, because step 3 uses an existing empty directory. (b) On macOS, the
   `mktemp -d` path under `/var/folders` canonicalizes to `/private/var/…`, so step 3's
   literal-path match fails on both Apple legs only. (c) A literal `<exe>/../..` join fails with
   `ENOTDIR` because the exe is a file. Step 3 also accepts any non-zero exit whose stderr names the
   path, including a crash after a log line. **Fix:** canonicalize only `current_exe()`, take
   `.parent().parent()`, and never canonicalize the override. Add context to a failing
   `current_exe`/`canonicalize` call, for example a binary replaced mid-run that shows as
   `(deleted)`. Add negative controls for a non-existent override, and for `lib/` removed with the
   override unset, which expects `<pwd -P scratch>/lib/xdu/extensions/httpfs.duckdb_extension`.
   Match the loader's own wording ("does not download") or a dedicated exit code.

6. **Medium: `install.sh` overwrites an existing extension in place.** On an upgrade, `cp` onto
   an existing `httpfs.duckdb_extension` rewrites the same inode. On macOS that invalidates the
   kernel's cached code signature for the file, so the next process that loads it is killed. On
   Linux, a running process that has it mapped can take SIGBUS. P4 installs into a fresh prefix and
   cannot see either. **Fix:** copy to a temporary name in the destination directory, then `mv`
   it over the old file, as the binaries are already moved today. P4 installs twice into the same
   prefix and runs `load` after the second install.

7. **Medium: the Dockerfile checks the builder's `/out`, not the image.** A wrong runtime `COPY`
   destination (`/usr/local/lib/xdu/`, `/usr/lib/…`) passes the builder-stage `check.sh`, and the
   image ships with tools that fail on their first load. Nothing runs in the runtime stage.
   **Fix:** use `COPY --from=builder /out/ /usr/local/` so the image tree is the checked tree.
   Then add a runtime-stage `RUN test -f /usr/local/lib/xdu/extensions/httpfs.duckdb_extension`,
   or run the probe in a throwaway stage built from the runtime base.

8. **Medium: the PR path filter misses the inputs that decide R6.** A PR that touches only
   `.cargo/config.toml` (the `-static-libstdc++` flag behind the linux_amd64 risk), `Cargo.lock`,
   `rust-toolchain.toml` or `src/engine.rs` runs no release legs, so the first evidence arrives at
   a real release. **Fix:** list `.cargo/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
   `src/engine.rs`, `examples/**`, `packaging/extensions/**` and the workflow file itself. Give PR
   runs `cancel-in-progress`, as `docker.yaml` does.

9. **Low: the `source_id` tie proves the version, not the build.** `6ddac802ff` is a compile-time
   constant in `pragma_version.cpp`, the same in every libduckdb-sys 1.4.4 build for every target
   and profile. A probe passed from the wrong directory (`target/release/examples` instead of
   `target/${T}/release/examples`, or a cached copy) still matches. **Fix:** call it a version tie
   in the plan. Pass the probe path from the same `target/${T}/release` expression that Assemble
   uses. Have step 1 compare the probe's architecture with `bin/xdu-find`'s (ELF `e_machine` or
   Mach-O `cputype`).

10. **Low: P3 can miss drift between the two jobs.** `build` and `build-macos` have separate step
    lists. **Fix:** for every job that uploads `tarball-*`, assert that check precedes upload,
    that the check step's tarball argument equals the upload `path`, and that neither the fetch
    step nor the check step carries `continue-on-error` or an `if:`.

11. **Low: `footer.sh` and its P2 tamper test.** Empty `ENGINE`/`PLATFORM` arguments fail today
    only because `"" != field`. A later `[ -z … ] ||` shortcut would let them pass. **Fix:**
    refuse empty arguments outright. In P2, tamper the version and magic fields as well as the
    platform, and add a file shorter than 512 bytes.

12. **Low: the version string.** DuckDB checks footers against `GetVersionDirectoryName()`. That
    equals `library_version` only for release-shaped tags; for a dev engine it is `source_id`. Today
    this is only a false-red risk (v1.4.4 is release-shaped). **Fix:** the probe refuses a
    `library_version` that is not release-shaped, rather than fetching a URL that 404s.

13. **Low: the HPCCM pin.** When `/xdu-release` moves `VERSION` to the first release with
    `lib/`, nothing asserts that the image carries the extension. **Fix:** in that same change, add
    `test -f {prefix}/lib/xdu/extensions/httpfs.duckdb_extension` to the existing `--version` smoke
    shell, and note the step in the release skill.

## Held up

- **Footer offsets.** Plain BSD `tail -c N | head -c 32 | tr -d '\000'` reads `4`, the platform,
  `v1.4.4`, `13f8a81` and `CPP` from all four official files. Exact equality rejects near-misses
  such as `linux_amd64_musl`. An empty, truncated or HTML body fails the magic field.
- **Legacy C++ ABI.** In 1.4.4, `platform.hpp` makes the legacy CXX11 ABI a `#error`, so a
  manylinux build cannot quietly report a `_gcc4` platform. `PRAGMA platform` is fixed at compile
  time by the target compiler, and all four legs are native.
- **Where DuckDB writes.** DuckDB core reads only `HOME`/`USERPROFILE` (plus `SLURM_*`) from the
  environment, with no XDG or TMPDIR paths. The secret manager only lists `~/.duckdb/stored_secrets`
  and creates it only when a persistent secret is written, so an `httpfs` LOAD does not write under
  `HOME`.
- **Signatures.** `allow_unsigned_extensions` stays off unless it is set explicitly, so a tampered
  file fails the signature check at step 2 and on operator hosts.
- **Autoload settings.** `enable_autoload_extension(false)` sets both `autoinstall_known_extensions`
  and `autoload_known_extensions` (crate `config.rs`).
- **R5 refusal test.** `json` is in `EXTENSION_TYPES`, so `'{}'::JSON` is a real autoload trigger.
  The autoload-off hint (`SET autoload_known_extensions=1`) differs from the wording of a failed
  autoinstall, and the observed-red requirement backs it. This holds as long as the test asserts
  that substring, not just `is_err()`.
- **Path resolution.** `pwd -P` in `install.sh` and the canonical exe agree under merged-`/usr`
  symlinks and a symlinked `XDU_INSTALL`. On the direct-load path DuckDB only expands a leading
  `~`, and the `'` refusal closes SQL injection.
- **PR security.** The `pull_request` trigger (not `_target`) runs with `contents: read`, and
  `publish` is gated on the event name. Caches written by a PR are not readable by tag runs.
- **Re-runs and old tags.** A `workflow_dispatch` against a tag from before this feature fails
  closed, because that ref has no probe or scripts. A re-run leg re-runs its own check, and
  `publish` still needs both jobs.
- **SHA-256 tool.** `sha256sum` exists on current macOS as well as Linux.
