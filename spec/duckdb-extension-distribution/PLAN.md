# PLAN — Ship DuckDB extensions instead of downloading them

> **Status:** Draft for review · **Last updated:** 2026-11-04
> **Authoritative technical design.** The *how*. Vision/contract is [`GOAL.md`](GOAL.md);
> the phased executable roadmap is [`TECH.md`](TECH.md). Backing detail is in
> [`research/`](research/). Every design element traces to a GOAL R-ID.

## 1. Summary

One new library module owns every DuckDB connection and every extension load. Connections open
with autoinstall and autoload switched off, so the offline guarantee becomes a refusal. Extensions
load only by absolute path from `<prefix>/lib/xdu/extensions/`, which is resolved beside the running
executable or overridden by `XDU_EXTENSION_DIR`. An absent file is an error that names the path. The
release legs fetch the pinned set (`httpfs`) for the exact engine version and platform the leg's own
engine reports. They check the file's footer against both, put it in the tarball, and then load it
from a scratch extraction under a cold `HOME` before anything is uploaded. Two shell scripts and one
Cargo example carry all of this, and the same scripts serve the `Dockerfile`. Four phases, no
reader query changes, no `duckdb` bump.

## 2. Design

### 2.1 The engine module (`src/engine.rs`, new; `pub mod engine;` in `src/lib.rs`)

- `PINNED_EXTENSIONS: &[&str] = &["httpfs"]` is the one statement of the pinned set (R1's "list").
  Everything downstream reads it: the loader, the probe, and through the probe the release scripts.
  Adding `aws` or `fts` later is one more entry here and nothing else.
- `open_connection() -> duckdb::Result<Connection>` builds
  `Config::default().enable_autoload_extension(false)?`, which sets both
  `autoinstall_known_extensions` and `autoload_known_extensions` to off, and opens in memory with it.
  `src/bin/xdu-find.rs:38`, `src/bin/xdu-rm.rs:58` and `src/bin/xdu-view.rs:1918` replace
  `Connection::open_in_memory()` with it: a one-line change each, and no SQL changes. `src/bin/xdu.rs`
  and `src/crawl.rs` link no DuckDB and are untouched.
- `EXTENSION_DIR_ENV = "XDU_EXTENSION_DIR"` and `EXTENSION_SUBDIR = "lib/xdu/extensions"`.
  `extension_dir_from(override, exe)` is a pure function. A non-empty override wins and is used
  **as given**: it is never canonicalized, because canonicalizing a missing directory replaces
  the path with a bare `os error 2` and breaks R4. Otherwise the result is
  `exe.parent().parent()` joined with `EXTENSION_SUBDIR`. A literal `exe/../..` join would fail
  with `ENOTDIR`, since the exe is a file. `extension_dir()` is the thin wrapper. It canonicalizes
  **only** `current_exe()`, so symlinked binaries resolve to the real tree, and it adds context
  when that fails (for example, a binary replaced mid-run). The pure split keeps the tests off
  `set_var`, which is `unsafe` in edition 2024 and racy under the parallel test harness.
- `clippy.toml` `disallowed-methods` names `duckdb::Connection::open`, `open_in_memory`,
  `open_with_flags` and `open_in_memory_with_flags`. `src/engine.rs` alone carries the `#[allow]`.
  The existing `clippy -D warnings` gate then stops any reader from opening a connection that
  skips the refusal. On the release artifacts, a regression to `open_in_memory()` would still load
  by path and still answer `--count`, so this rule is the only gate that sees it.
- `load_extension_from(conn, dir, name)` refuses a `name` outside the pinned set, and refuses when
  `dir/<name>.duckdb_extension` is not a regular file. That error names the full path, says xdu does
  not download extensions, and names `XDU_EXTENSION_DIR`. Otherwise it issues
  `LOAD '<absolute path>'`, adding the path as context to any DuckDB error such as a version or
  platform mismatch. The path can come from an environment variable, so it is an injection surface
  (§5). It is refused outright when it contains a `'` or is not UTF-8, rather than escaped, which
  adds no second escaping helper beside the `--pattern` one. `load_extension(conn, name)` composes
  `extension_dir()` with it.
- Nothing in a reader calls `load_extension`, which is how R5's "loads no extension it does not link
  statically" is met. The first caller is the S3 cycle.

### 2.2 The tarball layout (the new contract)

```
bin/{xdu,xdu-find,xdu-view,xdu-rm}
share/{man/man1/*.1, bash-completion/completions/*, zsh/site-functions/*}
lib/xdu/extensions/{name}.duckdb_extension     # one per pinned entry; this leg's engine + platform
```

The layout is flat, with no `<version>/<platform>/` levels. Each tarball is single-platform and
single-engine already, and the footer records both, so the extra directories would only add a second
place for them to disagree. Recorded in `AGENTS.md` (Packaging bullet, the §13 summary line, the
offline-test bullet) and in `.agents/factory/invariants.md` §13, in lockstep.

### 2.3 The probe (`examples/extension-probe.rs`, new)

This is a Cargo example, so `cargo install` never installs it (it installs every `[[bin]]`, which is
how `gen-completions` gets installed today). It has two modes and uses only `xdu::engine`:

- `manifest` prints `engine=` (`library_version` from `pragma_version()`), `platform=`
  (`PRAGMA platform`), `source_id=`, and one `extension=` line per pinned entry. It refuses, with a
  non-zero exit, an engine not shaped `vX.Y.Z`, a platform outside `{linux,osx}_{amd64,arm64}`, an
  empty `source_id`, or an empty set. The release scripts never hard-code a version, a platform
  or a name; the engine that was just built supplies them.
- `load` works on **one** `open_connection()` connection throughout, because each call opens a
  separate in-memory database. In order:
  1. Assert both autoload settings read false.
  2. Run `'{}'::JSON` and require DuckDB's autoload-disabled hint. That makes the refusal (R3, R5)
     an observation on the release artifact, not an inference from an empty `HOME`.
  3. Call `engine::extension_dir()`, then `load_extension` for each pinned entry, assert that
     `duckdb_extensions()` reports it loaded, and print `loaded=<name> <path>`.
  4. Run a `read_parquet` against a refused loopback port. The error must be `httpfs`'s own
     connection failure naming that URL, and any `autoload`, `INSTALL` or `requires the extension`
     wording is a failure. This crosses the engine/extension C++ boundary, which matters on
     `linux_amd64`, where the host may carry a static `libstdc++` copy
     ([research/01](research/01-r6-release-load-check.md)).
  It exits non-zero on any miss.

### 2.4 The scripts (`packaging/extensions/`, new, POSIX `sh`)

- `footer.sh FILE ENGINE PLATFORM` refuses empty arguments and a file shorter than 512 bytes. It
  reads the 512-byte footer: the magic field must be `4`, the platform field must equal
  `PLATFORM`, and the version field must equal `ENGINE`. It prints the extension version and
  SHA-256 for the log and exits non-zero on any mismatch. This is R1's static "match on either
  count".
- Both scripts below capture the manifest by **assignment** (`m=$("$PROBE" manifest)`), so a probe
  failure stops the script under `set -e`, which a `for x in $(…)` would hide. They re-validate
  the field shapes and require at least one `extension=` line. An empty or broken manifest
  therefore cannot become a leg that checks nothing and ships nothing.
- `fetch.sh PROBE DEST` downloads
  `https://extensions.duckdb.org/<engine>/<platform>/<name>.duckdb_extension.gz` for each entry
  with `curl -fsSL --retry 3`, gunzips it into `DEST/`, and runs `footer.sh` on it. Any failure is
  fatal.
- `check.sh TARBALL PROBE` is the R6 gate, run on an extraction in a self-cleaning `mktemp -d`
  (resolved with `pwd -P`). Every probe and reader run is `env -i PATH="$PATH" HOME="$cold"`.
  That way an exported `XDU_EXTENSION_DIR`, a proxy variable `httpfs` would honour, or the
  manylinux image's `LD_LIBRARY_PATH` cannot leak in, and the Linux legs load against stock RHEL8
  libraries. `$cold` must exist and be empty before the first run.
  1. The members include the four binaries, and the set of `lib/xdu/extensions/*` members is
     **exactly** the manifest's set. `footer.sh` re-runs on each extracted file.
  2. The probe goes into the scratch `bin/` and `load` runs there with no override, so the
     exe-relative resolution is what gets tested. Every `loaded=` path must be the scratch tree's
     `lib/xdu/extensions/<name>.duckdb_extension`.
  3. Three negative controls (R4). Each must exit non-zero with the loader's own "does not
     download" wording and the exact expected path:
     - an override pointing at an existing empty directory;
     - an override pointing at a directory that does not exist;
     - the scratch `lib/` removed, with no override.
  4. The engine-version tie: the manifest's non-empty `source_id` must occur in each of
     `bin/xdu-find`, `bin/xdu-view` and `bin/xdu-rm` (`grep -a -F`). The constant is shared by
     every build of one engine version, so this proves the tarball binaries carry the probe's
     engine **version**. That they come from the same build is structural: one `cargo build`
     invocation, and the workflow passes the probe from `target/<target>/release/examples/`.
  5. The shipped readers: the extracted `bin/xdu` indexes a three-file tree. Then, under the cold
     `HOME`, the extracted `bin/xdu-find --count` must print `3` and `bin/xdu-rm --pattern '*.log'
     --dry-run` must report its two matches, as `tests/offline_tests.rs` does.
  6. The cold `HOME` is still empty.

### 2.5 Release and CI workflows

- `.github/workflows/release.yaml`, both build jobs:
  - The build becomes `cargo build --release --locked --target T --bins --example extension-probe`,
    one invocation, so the probe and the tarball binaries share every compiled artifact.
  - A **Fetch DuckDB extensions** step after the build (and on Linux after the glibc-floor step)
    runs `fetch.sh` into `staging/lib/xdu/extensions`.
  - **Assemble** now tars `bin share lib`.
  - A **Check extensions load (cold HOME)** step runs `check.sh` on the assembled tarball **before**
    the upload step. A failing leg uploads nothing, and `publish` needs every leg, so nothing is
    published (R1's "fail rather than publish"). The Linux legs run this inside manylinux_2_28 (glibc
    2.28, RHEL8 libstdc++), which is the observation that closes spike claim 13.
  - The Apple legs move off the retired `macos-14` label: `aarch64-apple-darwin` to `macos-15`, and
    `x86_64-apple-darwin` to `macos-15-intel`, which makes that leg native so its check needs no
    Rosetta.
  - Neither the fetch step nor the check step carries an `if:` or `continue-on-error`, and the
    check step names the same tarball path as the upload.
  - A `pull_request` trigger runs `prepare` and all four legs. Its `paths` are every input that
    decides R6: `.github/workflows/release.yaml`, `packaging/extensions/**`, `examples/**`,
    `src/engine.rs`, `.cargo/**` (the `-static-libstdc++` flag), `Cargo.toml`, `Cargo.lock` and
    `rust-toolchain.toml`. `publish` gains `if: github.event_name != 'pull_request'`. `TAG` falls
    back to `pr<number>` on a PR so artifact names stay slash-free. `cancel-in-progress` becomes
    true for PR runs only, so a release run is still never cancelled. This is how R6 is observed on
    all four legs before merge instead of first at a real release.
- `.github/workflows/test.yaml`: `macos-14` becomes `macos-15` (same retirement).
- `.github/workflows/docker.yaml`: the PR `paths` filter gains `examples/**` and
  `packaging/extensions/**`.

### 2.6 Install channels (R2)

- `install.sh`: when the tarball carries `lib/xdu/extensions/`, it installs the files into
  `$(cd "$INSTALL_DIR" && pwd -P)/../lib/xdu/extensions`. The path is relative to where the
  binaries went, not to `XDU_PREFIX`, because `XDU_INSTALL` may diverge from `$PREFIX/bin`. It is
  the physical path, so a symlinked install directory lands where the canonicalized exe looks.
  Each file is copied to a temporary name in the destination directory and then `mv`'d over the
  old one, the way the binaries already are. An in-place `cp` over an existing extension would
  invalidate macOS's cached code signature, so the next load is killed, and can `SIGBUS` a running
  Linux process. An older `XDU_VERSION` whose tarball has no `lib/` installs as before.
- HPCCM (`hpccm/xdu.py`) already extracts the whole tarball into the prefix, so `lib/` lands beside
  `bin/` by construction. Only the docstring changes, plus `make check`. A `test -f` guard cannot
  land yet: the recipe pins `VERSION = '0.5.2'`, which has no `lib/`, so the guard would break the
  recipe on `main` until the next release. It is deferred to the release that first moves the pin
  to a tarball carrying `lib/`. Its record is a new `issues/` file plus a ROADMAP entry (P4 ledger).
- `Dockerfile`: the builder installs `curl`, copies `examples/` and `packaging/extensions/`, builds
  the probe in the same `cargo build`, stages `/out/{bin,lib/xdu/extensions}`, and runs `fetch.sh`
  and then `check.sh` on a tarball of that staging, so a bad image fails its build. The runtime
  stage takes the checked tree whole (`COPY --from=builder /out/ /usr/local/`), so the image tree
  is the tree that was checked. It then runs `test -f /usr/local/lib/xdu/extensions/httpfs.duckdb_extension`,
  so a wrong destination fails the image build.
- `README.md` Installation gains three lines: where extensions live, `XDU_EXTENSION_DIR`, and that
  `cargo install` builds carry none.

### Requirement → design map

| R-ID | Design element(s) that satisfy it |
|------|-----------------------------------|
| R1   | `PINNED_EXTENSIONS` (the list); `fetch.sh` + `footer.sh` pin to the leg engine's own `engine`/`platform`; `check.sh` re-checks footers in the tarball; the check runs before upload, and `publish` needs all four legs |
| R2   | §2.2 layout recorded in `AGENTS.md` + `invariants.md`; `install.sh` exe-relative placement; HPCCM by construction; `Dockerfile` places and checks `/usr/local/lib/xdu/extensions` |
| R3   | `engine::open_connection` (autoinstall/autoload off); `load_extension` by absolute path from `extension_dir()` (exe-relative, `XDU_EXTENSION_DIR` override); no write under `$HOME` asserted by `check.sh` |
| R4   | `load_extension_from` refuses an absent file with the full path, before any `LOAD`; unit test plus `check.sh` negative control |
| R5   | all three readers open through `open_connection`, enforced by `clippy.toml` `disallowed-methods`; no reader calls `load_extension`; refusal unit test (local `json` trigger fails, no install), repeated by the probe on every release leg; `tests/offline_tests.rs` unchanged and green |
| R6   | `check.sh` on every leg, on the extracted tarball, under a cold `HOME` with autoinstall off, via a probe from the same build; Linux legs inside manylinux_2_28; run on PRs |

## 3. Invariant gate (AGENTS.md constitution check)

Gate #1 (before research) found the intent sane against §5, §6, §10, §11 and §13, and found nothing
fundamentally at odds. Gate #2 (this design):

- **§1 schema / §2 / §2b / §3 / §4 / §7 / §8 / §9 / §12:** untouched. No SQL, schema, layout, marker,
  crawl or TUI-safety change. `xdu-rm.rs` and `xdu-view.rs` change only how the connection opens.
- **§4 `xdu-rm` safety:** the one-line connection change in `src/bin/xdu-rm.rs` leaves the
  confirm, dry-run, `--safe` and `--limit` paths untouched. `rm_tests.rs` must stay green.
- **§5 injection:** the only new value reaching SQL is the extension path, which can come from an
  environment variable. It is refused when it contains `'` or is not UTF-8, so it never needs
  escaping. Names are checked against `PINNED_EXTENSIONS` before use.
- **§6 Unix-only:** `current_exe` + `canonicalize` behave on Linux and macOS. Nothing new assumes
  portability.
- **§10 CLI single source:** no flag, subcommand or `.scd` change. `XDU_EXTENSION_DIR` is read by the
  library, not declared through clap, and no man page gains a literal, so the literal-count list
  is untouched. Its man-page entry is deferred to the first CLI consumer (§5 below).
- **§11 altitude:** all logic lives in `src/engine.rs`. The probe and the bins stay thin. The new
  `clippy.toml` rule keeps connection-opening in that one module.
- **§13 conventions:** the tarball-layout contract changes, and is updated in `AGENTS.md` (two
  places) and `invariants.md` §13 in the same commit as `install.sh`. No version is hard-coded.
  `rm` survives only where AGENTS.md keeps it (CI, `install.sh`, `Dockerfile`, the
  self-cleaning `mktemp -d` in `check.sh`). No `spec/` ids go into source. The pre-release gate
  (fmt, clippy `--all-targets`, which now covers the example, and test) stays clean.

### Deviation justifications

| Deviation | Why needed | Simpler alternative rejected because |
|-----------|-----------|--------------------------------------|
| **[QUESTION — touches R6's wording]** R6's load is performed by a probe (`examples/extension-probe`) built in the same `cargo build` as the tarball binaries and run from the extracted tarball's `bin/`, not by the four tarball binaries themselves. The shipped binaries are tied in structurally (one `cargo build` invocation, probe taken from that leg's `target/<target>/release/examples/`), by the engine-version `source_id` check, and by driving the extracted `xdu`/`xdu-find`/`xdu-rm` under the same cold `HOME`. | No shipped binary can load an extension this cycle without breaking another commitment: the loader is library-only (Clarification 3, non-goal "operator-visible diagnostic") and the reader path must load nothing (R5). | (a) A hidden flag or subcommand in a shipped binary is an operator-visible diagnostic and drags in §10's man-page obligation. (b) An env-var hook that makes a reader load the set puts a non-static load on the local query path, against R5. If the foreman reads R6's "the binaries in that leg's tarball" as satisfied by the same-build engine run from the tarball layout, no GOAL change is needed; if not, R6 or Clarification 3 has to move. |
| `release.yaml` gains a path-filtered `pull_request` trigger; `publish` is skipped on PRs; `TAG` gets a PR fallback. *Does not change any R-ID.* | Without it R6 and R1 run for the first time at a real release, and the spike's claim 13 (the one R6 exists to close) is never observed before merge. No container runtime on the planning host can stand in for the Linux legs. | A `workflow_dispatch` dry-run input checks out `inputs.tag`, so it would build the tag rather than the branch, and dispatching it is an outbound action the build step cannot take on its own. The `docker.yaml` PR guardrail is the precedent followed. |
| Apple legs and the `test.yaml` macOS host move from `macos-14` to `macos-15` / `macos-15-intel`. *Does not change any R-ID; not a new leg.* | `macos-14` retired from GitHub Actions on 2026-11-02, so those jobs currently get no runner and R6 cannot run on them. | Staying on `macos-14` leaves R1 and R6 unrunnable on two of four legs. Cross-building `x86_64-apple-darwin` on arm64 would make its check depend on Rosetta, which the runner images do not document. |

## 4. Rabbit holes (resolved)

- **Does a stripped, LTO'd release binary export what `httpfs` needs?** It needs nothing: the
  extension imports no `duckdb::` symbols on Linux or macOS
  ([research/01](research/01-r6-release-load-check.md)).
- **How does a leg get a matching binary per platform?** From the extension repository, at the
  engine version and platform the leg's own engine reports, so no table maps Rust triples to DuckDB
  platforms. All four URLs answer for v1.4.4, and `linux_amd64_gcc4` no longer exists.
- **How is "matches on either count" checked without DuckDB tooling in the shell?** The 512-byte
  footer has fixed 32-byte fields at fixed offsets from the end, guarded by a magic field
  (research/01 has the offsets, confirmed on two platforms).
- **Does the Linux `httpfs` fit the manylinux_2_28 floor?** On symbols, yes (`GLIBC_2.28`,
  `GLIBCXX_3.4.22`). The R6 check in the manylinux container is the observation.
- **Is the refusal testable without the network?** Yes. `json` is in the amalgamation but not linked,
  so `'{}'::JSON` is a purely local autoload trigger.
- **Adversarial review of the gate design:** [research/02](research/02-adversarial-review.md),
  13 findings, two of them high. All were folded into §2.1–§2.6 and the gate scripts, except the
  HPCCM guard, which is deferred (§2.6). [research/00](research/00-digest.md) maps each finding
  to where it landed.

## 5. Risks & open questions

- **R6's subject** (deviation row 1) needs the foreman's reading before build.
- **When the Linux evidence exists.** The two manylinux legs first run on the pull request.
  `xdu-review` grades R6 from the macOS-host run in P2 plus the PR run's per-leg logs. Pushing the
  branch and opening that PR is a human-approved act, not something a build phase does.
- **Mixed C++ runtimes on `linux_amd64`.** The `httpfs` boundary query in `load` is the mitigation.
  If it aborts in the manylinux leg, the fix is in `.cargo/config.toml`, which gets its own decision
  rather than a silent edit.
- **The `source_id` tie is weaker than it looks.** It is constant across builds of one engine
  version, so it proves the version, not the build. The build identity rests on the workflow's
  structure, which P3's gate asserts. If the string does not survive `strip` + LTO on the host
  build, P2 stops and escalates rather than dropping the check.
- **Upstream republish.** A re-run of the same tag may fetch a newer `extension_version` for the
  same engine. Both counts the GOAL pins still match, the signature is still checked at `LOAD`, and
  the log records `extension_version` and SHA-256. Hash-pinning was rejected, because it would make
  "add an extension" four hashes per entry and turn an upstream republish into a red release.
- **Channels outside R2's list.** `cargo install` and the RPM spec build from source and carry no
  extensions. Recorded: README (cargo), `issues/native-os-packages-deb-rpm.md` (RPM/deb inherit
  `lib/xdu/extensions/`), and `issues/s3-index-target.md`, which gets the
  `XDU_EXTENSION_DIR` man-page entry when the first CLI path loads an extension. All three
  destinations already have ROADMAP entries.
- **`xdu-view`'s SQL is not driven offline** (TUI). It uses only `read_parquet` and `regexp_extract`,
  both statically linked, so turning autoload off cannot remove anything it relies on.

## 6. Verification strategy

- **P1:** unit tests in `src/engine.rs`. Both settings report off on an `open_connection`
  connection. The refusal: `'{}'::JSON` errors with DuckDB's autoload-disabled wording while a
  temporary `extension_directory` stays empty. That test must be observed red against a plain
  `open_in_memory()` connection before the fix lands. An absent file gives an error naming the full
  path, with the directory untouched. A name outside the set and a quoted path are refused, and
  path resolution is checked. Then `tests/offline_tests.rs` (cold `HOME` stays empty) and
  `rm_tests`/`find_tests` stay green.
- **P2:** on the host leg, the real release-profile build of the bins and the probe, then
  `fetch.sh`, a tarball assembled the way the workflow does it, and `check.sh` on it. The check is
  re-run with a hostile environment (a bogus `XDU_EXTENSION_DIR` and proxy exported), which it must
  still pass. Several controls must go red:
  - `footer.sh` on the platform, version and magic fields rewritten, on a short file, on a
    non-extension, on a wrong engine, and with empty arguments;
  - `check.sh` on a tarball without `lib/`, on one with an extra extension, and on one carrying a
    tampered file.
- **P3:** structural assertions on the parsed workflow YAML: step order fetch → assemble → check →
  upload, `lib` in the tar members, no `if:`/`continue-on-error` on fetch or check, the same
  tarball path in check and upload, the probe taken from `target/<target>/release/examples/`,
  runner labels, the publish guard, the PR trigger's paths and its PR-only cancellation. R6's
  four-leg evidence is the PR run.
- **P4:** `install.sh` driven end to end through a `curl` shim that serves the P2 tarball and its
  `SHA256SUMS`, in three layouts: `XDU_INSTALL` outside `XDU_PREFIX`, a second install over the
  first (the upgrade path), and a symlinked install directory. After each, the probe, copied
  beside the installed binaries, `load`s under a cold `HOME`. Plus `make check` for HPCCM, and
  static checks of the `Dockerfile` (its build, including the runtime `test -f`, is exercised by
  `docker.yaml` on the PR).

---

*Backing research: [`research/00-digest.md`](research/00-digest.md).*
