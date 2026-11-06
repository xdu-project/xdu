# Research 05 — CI, docs and test harness for the S3 index target

Track: R7–R10. Sources: `.github/workflows/{test,release,docker}.yaml`, `packaging/extensions/`,
`src/engine.rs`, `tests/common/mod.rs`, `tests/offline_tests.rs`, `doc/*.1.scd`, `README.md`,
`AGENTS.md` Commands, `invariants.md` §10/§13, `Dockerfile`, the archived brief
`archive/s3-index-target-p2:…/research/05-cli-and-tests.md`.

## 1. The pinned `httpfs` in tests

- **Nothing provides it to `cargo test` today.** The only fetch is
  `packaging/extensions/fetch.sh PROBE DEST` (`fetch.sh:28,49-53`): it reads engine, platform and
  set from `extension-probe manifest`, curls `extensions.duckdb.org/<engine>/<platform>/<name>.gz`,
  and checks the file with `footer.sh` (magic, engine, platform). That check stands in for a
  checksum; no pinned hash, manifest file or cache exists. Callers: release legs
  (`release.yaml:190-191,271-272`) and `Dockerfile:65`. `test.yaml` never fetches.
- **No test LOADs a real `httpfs`.** `src/engine.rs` tests use absent or garbage files only. The
  spike loaded it under the debug test profile (`bench/spikes/run.sh:43`, osx_arm64). A
  debug-profile load on linux_amd64 has not been observed. The release check only proves release
  builds.
- **Default lookup under `cargo test`:** `<canonical exe>/../../lib/xdu/extensions`
  (`engine.rs:58-71`), and the exe is `target/debug/xdu-find`, so the lookup lands in
  `target/lib/xdu/extensions`. It is absent in this worktree.
- **Recommendation.** Fetch into a **non-default** dir, `$RUNNER_TEMP/xdu-ext`, and pass it to tests
  through a test-only variable such as `XDU_TEST_EXTENSION_DIR`. Each S3 test then sets
  `XDU_EXTENSION_DIR` itself, per command. Do not fetch into `target/lib/…`, because then a reader
  that wrongly loads `httpfs` for a local index passes silently (see §2). CI step, both legs, after
  Build:
  `cargo build --locked --all-features --example extension-probe`, then
  `sh packaging/extensions/fetch.sh target/debug/examples/extension-probe "$RUNNER_TEMP/xdu-ext"`.
  ubuntu-24.04 maps to linux_amd64 and macos-15 to osx_arm64, both on fetch.sh's allowlist. CI has
  network, and this is an install-time fetch, like `apt-get`/`pipx` in the packaging job; the tests
  themselves stay on loopback.
- **Developer without it:** the same two commands work locally. Tests that need the extension skip
  loudly (§2).

## 2. Harness conventions and the skip story

- `tests/common/mod.rs` is the one home for helpers. `binary_path` uses `CARGO_BIN_EXE_*`
  (`:22-30`). There are `run_xdu/find/rm/view` (`:122-190`), `build_index` (`:137`, always
  `--apparent-size`), `run_binary_with_home` (`:194-205`, sets only `HOME`) and
  `list_files_recursive`. Children inherit the whole environment. S3 helpers belong here, never in
  a test file (AGENTS.md:512-519).
- `temp_index.sh` builds a local index only (`:49-57`). Do not put S3 into it; at most add a sibling
  helper.
- **Existing skip pattern:** `eprintln!("SKIP …"); return` (`find_tests.rs:104-122`,
  `crawl_tests.rs:482…`), which reports `ok` (AGENTS.md:520). That fails R10's "never pass silently".
  **Recommend `#[ignore = "needs loopback S3 endpoint + httpfs; see …"]`** on endpoint tests. A
  plain `cargo test` then prints them as *ignored*, the harness's own skip report. CI runs
  `cargo test --locked --all-features --test s3_tests -- --ignored` in a dedicated step. Inside,
  a missing endpoint or extension variable **panics**, so an opted-in run can never skip.
  No `#[ignore]` exists in the repo today, so `--ignored` scoped to that file is unambiguous.
- **Tier the tests by need:**
  - **Need nothing:** run in plain `cargo test`.
    - R8: `xdu-rm -i s3://…` refuses.
    - R7 absent extension: point `XDU_EXTENSION_DIR` at an empty temp dir; the error names the
      path and both remedies.
    - A local-index query with `XDU_EXTENSION_DIR` pointing at a dir holding a *garbage*
      `httpfs.duckdb_extension` must still succeed. This proves the local path loads nothing.
  - **Need the extension only:** R4 unreachable endpoint (`127.0.0.1:9`, as
    `examples/extension-probe.rs:39`).
  - **Need the extension and the endpoint:** R1–R3 and the other R4/R5 cases.
- **Hermeticity for every S3 child process.** `env_remove` all `AWS_*`. Point `HOME` at a temp dir,
  and point `AWS_SHARED_CREDENTIALS_FILE`/`AWS_CONFIG_FILE` at fixtures, so a developer's real
  profile is never read. Clear `HTTP(S)_PROXY`/`ALL_PROXY` (both cases) or set
  `NO_PROXY=127.0.0.1,localhost`. Agent sandboxes export proxy variables, and `check.sh:16` already
  treats them as able to sway `httpfs`. R7's "no write under `$HOME`" can reuse the offline pattern:
  cold `HOME`, then assert `list_files_recursive(home).is_empty()`.
- **`offline_tests.rs` (R7).** It builds a local index and runs `xdu-find --count` and
  `xdu-rm --dry-run` under a cold `HOME` that must stay empty (`:31-89`). It breaks if:
  - a reader loads `httpfs` eagerly. The default dir is absent under `cargo test`, so it fails red.
    If CI has fetched into `target/lib/…`, it goes **green and hides the defect**, hence §1.
  - credentials resolve at startup for every locator, e.g. an SDK default chain doing IMDS calls
    (hangs on timeouts) or writing an SSO/token cache under `HOME`.
  - the S3 client is constructed for a local path.

  Rule: dispatch on the locator first, and resolve credentials and load the extension only on the
  `s3://` arm.

## 3. Man-page gate

- **Mechanics** (`test.yaml:168-247`). `published()` runs `mandoc -Tutf8 | col -b | tr -d
  '[:space:]'`. `check PAGE SPEC…` refuses a page rendering to fewer than 200 chars. `Nx:LIT` means
  exactly N (`:205-212`, multi-digit N allowed). A bare spec is a presence check. Today's calls
  (`:226-239`):
  - `xdu.1`: six literals, `2x:.partial suffix` among them.
  - `xdu-find.1` and `xdu-view.1`: `2x:XDU_INDEX`.
  - `xdu-rm.1`: `2x:XDU_INDEX`, `2x:XDU_JOBS`.

  The line-start rule is `grep -nE "^[[:space:]]*['.]" doc/*.scd` (`:242`).
- **`ENVIRONMENT` today:**
  - `xdu.1:63-67`: `XDU_JOBS`. It is cross-referenced at `:34`, so 2x, but unasserted (an existing
    gap).
  - `xdu-find.1:90-94`: `XDU_INDEX` (2x with `:26`).
  - `xdu-view.1:138-142`: `XDU_INDEX` (2x with `:27`).
  - `xdu-rm.1:84-91`: `XDU_INDEX` and `XDU_JOBS` (2x each).

  No page mentions `s3`, `AWS`, `extension` or `cargo` today.
- **Proposed literals.** Derive the final N from the final text; the plan should fix the text so
  the counts are knowable.
  - `xdu.1`, `xdu-find.1`, `xdu-view.1`:
    - `Nx:s3://BUCKET/PREFIX` (in OPTIONS `-o`/`-i`, and EXAMPLES if shown).
    - Each of the ten `AWS_*` names.
  - `xdu-find.1`, `xdu-view.1` add:
    - `2x:XDU_EXTENSION_DIR` (the `ENVIRONMENT` entry plus the cross-reference in the remedy).
    - `lib/xdu/extensions` and `cargo install`.
  - `xdu.1`: optionally close the gap with `2x:XDU_JOBS`.

  Use `1x:` rather than presence for a literal meant to be unique. A later duplication then fails
  loudly and forces a re-count.
- **Hazards:**
  - (a) **Substring overlap:** `AWS_ENDPOINT_URL` is a prefix of `AWS_ENDPOINT_URL_S3`. Its count
    is n(URL) + n(URL_S3), so write the sum and comment on it. No other pair in the set nests.
  - (b) **Count bumps:** an S3 example such as `XDU_INDEX=s3://… xdu-find` raises `XDU_INDEX` to
    3x, and the `check` line must change in the same commit.
  - (c) **Escaping:** never write `AWS_*` or any other bare `*` in prose (use `\*`, or list the
    names). Mid-word `_` is safe. `s3://_BUCKET_/_PREFIX_` underlines like `_OUTDIR_/…`.
  - (d) **Line starts:** no wrapped line may start with `.` (`.aws/config`, `.xdu-complete`); write
    `~/.aws/config` and rewrap.
  - (e) **Rendering:** scdoc is not installed in this worktree's environment, so render on CI or
    after `brew install scdoc`.
- **§13's "one rule in three places".** The normalization does not change. Invariant §13:248 still
  says that adding or re-counting a literal carries the three-place obligation. In practice that
  means one edit: extend AGENTS.md:277's sentence ("every `*XDU_INDEX*` / `*XDU_JOBS*` … appears
  twice") to name `*XDU_EXTENSION_DIR*`, in the same commit. No invariant text enumerates literals.

## 4. README

- `README.md:89-91`: extensions sit at `../lib/xdu/extensions/`, overridable by
  `XDU_EXTENSION_DIR`, and nothing is downloaded.
- `README.md:99-101`: `cargo install` carries none; copy them from a tarball or set the variable.
  This matches R7's remedy text, but it does not yet say which features need an extension (S3
  indexes).
- Put the R9 example in a new `### Indexes in S3` section before `### Querying with DuckDB`
  (`:379`). It should cover: `xdu … -o s3://bucket/prefix`, and `xdu-find -i s3://…` /
  `XDU_INDEX=s3://… xdu-view`; the `AWS_*` and endpoint sources; and the fact that `xdu-rm` refuses
  `s3://`. Add one clause at `:99-101`: "S3 indexes need `httpfs`".
- Nearby drift, not this cycle:
  - `:99` says "nightly", but the toolchain is pinned stable.
  - `:116` uses `-b 100000`, but the flag is `-B`.

## 5. `xdu-rm` (R8)

R9 does not require a line on the `xdu-rm` page. I recommend one anyway, for two reasons:

- `XDU_INDEX` is shared, so an operator who exports an `s3://` value for `xdu-find` hits the refusal
  in `xdu-rm`.
- §10 ("a CLI change updates the affected `.scd`") can be read as covering a change to what `-i`
  accepts.

Add one sentence under `-i`: "an `s3://` locator is refused". If it names `XDU_INDEX`, re-count
`2x:XDU_INDEX`. An optional assert: `1x:s3://`.

## 6. CI shape

- **Test job** (`test.yaml:56-89`): matrix `[ubuntu-24.04, macos-15]`, `fail-fast: false`, 60-min
  timeout, `RUSTFLAGS=-D warnings` only. That variable overrides `.cargo/config.toml`'s
  static-libstdc++ flag in tests. Steps: Build (`:85`), Test (`:88`). No other environment, no
  services, no uv.
- **Endpoint placement:** add steps between Build and Test on both legs:
  - fetch extensions (§1);
  - start the endpoint in the background, poll for readiness, export the endpoint address,
    `XDU_TEST_EXTENSION_DIR` and dummy keys through `$GITHUB_ENV`;
  - after Test, run a separate "S3 tests" step with `-- --ignored`.

  Containers are out (R10, and macos-15 has no Docker). The two candidates:
  - **(a) The MinIO server binary**, pinned by release and sha256 per OS-arch. This is the spike's
    store. Before relying on it, check whether the community edition still publishes binaries.
  - **(b) `moto[server]`**, pinned, run as `uvx --from 'moto[server]==X' moto_server -p PORT`. This
    is one command on both legs and on developer machines. It supports ranged GET, ListObjectsV2
    and multipart.

  uv is not set up in any workflow today; add `astral-sh/setup-uv`, SHA-pinned per the repo's
  action convention, or use `pipx run` (pipx is already used at `test.yaml:262`). Python is on both
  images if the R3 byte count reuses a logging proxy like `bench/spikes/proxy.py`. An in-test Rust
  proxy over `std::net` avoids a second process.
- **Build legs a new dependency must pass:**
  - `release.yaml` builds in `manylinux_2_28` containers (x86_64 and aarch64; `:103-121`), with no
    cargo cache and a 90-minute timeout. It asserts the GLIBC 2.28 / GLIBCXX 3.4.25 / GCC 8.0.0
    ceilings and a **NEEDED allowlist**: libc, libm, libdl, libpthread, libgcc_s, libstdc++,
    ld-linux (`:160-186`). A dynamically linked `libssl` from `openssl-sys` fails that allowlist, so
    use rustls.
  - `aws-lc-sys` may want `cmake`, and that image's tooling is unverified. `ring` needs only `cc`.
  - These legs run on any PR touching `Cargo.toml`/`Cargo.lock`/`src/engine.rs` (`:16-25`).
  - `docker.yaml:26-33` builds `Dockerfile` (`rust:1-slim-bookworm` with only `g++ curl`,
    `:23-29`) on the same triggers. A crate needing `cmake`, `perl` or `pkg-config` breaks it.

## Change map

| file | change | R |
|---|---|---|
| `.github/workflows/test.yaml` | test job: fetch ext to `$RUNNER_TEMP`, start endpoint, `--ignored` S3 step; packaging gate: new literals/counts | R10, R3, R9 |
| `tests/common/mod.rs` | S3 env helper (scrub `AWS_*`/proxy, temp HOME, fixtures), endpoint/ext getters that panic when opted-in | R10, R5 |
| `tests/s3_tests.rs` (new) | tiered tests; endpoint tier `#[ignore]` | R1–R8 |
| `tests/offline_tests.rs` | unchanged; add sibling "local query ignores a garbage httpfs" test | R7 |
| `doc/xdu.1.scd`, `doc/xdu-find.1.scd`, `doc/xdu-view.1.scd` | `s3://` locator, `AWS_*` ENVIRONMENT; find/view: `XDU_EXTENSION_DIR` + `cargo install` remedy | R9, R7 |
| `doc/xdu-rm.1.scd` | one refusal sentence (recommended) | R8 |
| `AGENTS.md:277` | name `XDU_EXTENSION_DIR` among counted env literals | R9 (§13) |
| `README.md` | `### Indexes in S3` before `:379`; clause at `:99-101` | R9 |

## Risks

- Loading `httpfs` from a debug build on linux_amd64 has never been observed. Prove it on the first
  CI push.
- The endpoint binary may be unavailable or behave differently (MinIO distribution; moto's
  fidelity on ranged reads). R3's bound is measured at the client, so either works if Range is
  honored.
- Proxy variables in a developer's environment can silently route loopback traffic.
- If the extension is fetched into `target/lib/…`, the offline test can no longer catch an eager
  load.
- Count drift in the man-page gate, and the `AWS_ENDPOINT_URL` substring overlap.
- An S3 crate that breaks the manylinux allowlist or the Dockerfile's minimal builder only shows
  up on those PR-triggered workflows, not in `test.yaml`.
