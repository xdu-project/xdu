# REVIEW — Ship DuckDB extensions instead of downloading them

> Adversarial QA by `xdu-review`, run in an isolated/clean context. The correctness pass grades the
> branch diff against [`GOAL.md`](GOAL.md) + the AGENTS.md invariants **only** — it does not see
> `PLAN.md`/`TECH.md` (avoids grading-its-own-homework / plan-sycophancy). Every finding cites an
> **executed** command, not an assertion.

- **Reviewed commit:** 9cd2582  ·  **Base:** main (b47ba2b)  ·  **Date:** 2026-11-05
- **Verdict:** changes-requested
- **Cycle:** 1 of ≤3 — mirrors `review.cycle` in `TECH.md` (escalate to human on non-convergence)

One blind correctness reviewer, given `GOAL.md`, the spec-excluded diff
(`git diff main...HEAD -- . ':(exclude)spec/'`), `invariants.md`, `review-rubric.md` and the
runnable repo. No `debate` or `completeness` argument was passed, so neither variant ran. The
reviewer left `git status --porcelain` empty and HEAD at 9cd2582. It ran every negative control
on copies, so no `target/` artifact was mutated; the throwaway-index drive rebuilt the host
release binaries from HEAD sources, which is an ordinary build.

**Contract drift.** `GOAL.md` has three commits on the branch: the shaping commit (1615832), the
clarification answers (8f7e96c), and f3f09c2, which amends R6 to load the extension into a
same-build engine rather than a shipped binary. f3f09c2 landed at the PLAN gate, before the first
build commit (acfcad2), and is recorded as a dated Clarification attributed to the gate judge.
This review grades the amended R6. The amendment was not re-confirmed with a human in this
session; it is listed here so the publish gate sees it.

## Verification run

Reviewer (blind), on this macOS arm64 host:

- `cargo fmt --all -- --check` → clean.
- `cargo clippy --offline --locked --all-targets --all-features -- -D warnings` → clean.
- `cargo test --offline --locked` → all pass: lib 101, xdu-view 5, cli_version 1, crawl 25, find 9,
  offline 1, rm 18, version 6.
- `clippy.toml` negative control: a raw `Connection::open_in_memory()` in a scratch example fails
  clippy with "use of a disallowed method".
- Autoload control under a cold `HOME`: on a plain `open_in_memory()` connection, a
  `'{}'::JSON` cast reaches for `extensions.duckdb.org` and creates `v1.4.4/osx_arm64/` in the
  extension directory. On `engine::open_connection`, the same cast fails with the
  `SET autoload_known_extensions=1` hint and writes nothing.
- Full `osx_arm64` leg, run locally:
  - `cargo build --release --locked --target aarch64-apple-darwin --bins --example extension-probe`
  - `extension-probe manifest` → `engine=v1.4.4 platform=osx_arm64 source_id=6ddac802ff
    extension=httpfs`
  - `fetch.sh`, then `tar … bin share lib`, then `check.sh` → `extension-check: ok
    platform=osx_arm64 engine=v1.4.4`, rc 0.
- `footer.sh` refuses each mismatch, rc 1:
  - a `linux_amd64` file → `built for platform 'linux_amd64', expected 'osx_arm64'`
  - a `v1.4.3` file → `built for engine 'v1.4.3', expected 'v1.4.4'`
  - a junk file → `footer magic is not 4`
- `check.sh` refuses each bad tarball, rc 1: a `linux_amd64` httpfs, a `v1.4.3` httpfs, an extra
  `aws.duckdb_extension`, and no `lib/`.
- `fetch.sh` refuses each bad fake-probe manifest: a duplicate `engine=`, platform
  `linux_amd64_musl`, extension `../evil`, no extensions, and a probe that exits non-zero.
- Extension-directory drives with `extension-probe load` under a cold `HOME`, all as specified:
  - an absolute `XDU_EXTENSION_DIR` override
  - a missing default directory, which fails naming
    `…/lib/xdu/extensions/httpfs.duckdb_extension; xdu does not download extensions`
  - a symlinked executable, which resolves to its real tree
  - an empty override, which is ignored
  - a missing override directory, which wins and errors
  - a relative override, which fails: see F3
- `install.sh` with a local `curl` stand-in and no network:
  - a plain `XDU_INSTALL`, a symlinked install directory, and a re-install
  - extensions land beside the physical `bin/`, and a re-install leaves exactly one file
  - a probe placed in each installed `bin/` loads from there under a cold `HOME`
  - The run needed a harness-only `mktemp` stand-in: that is the environment, not the product.
- HPCCM: specs regenerated with `uvx hpccm` → byte-identical to the committed `xdu.def` and
  `xdu.docker`.
- `temp_index.sh` drives under a cold `HOME`:
  - `xdu-find --count`, `--top`, `-p '*.log'`, `--min-size`, `--newer-than` all behaved.
  - `xdu-rm -p '*' --dry-run --limit 2` selected the same two files that `--force --limit 2` then
    deleted.
  - The SQL functions the readers use all run on an `open_connection` connection (`regexp_extract`,
    `regexp_matches`, `bool_or`, `position`, `substr`, `length`), as do `xdu-view`'s CTE and
    partition query shapes.
  - The cold `HOME` stayed empty.
- All three workflow YAMLs parse.

Orchestrator:

- `sh spec/duckdb-extension-distribution/verify/p3.sh` → `P3-GATE-GREEN`. It checks the workflow
  structure and the committed four-leg evidence in `evidence/p3-pr-run.md`, which names head
  ab0f7d9, an ancestor of HEAD.
- `git diff --stat ab0f7d9 HEAD -- . ':(exclude)spec/'` → 12 files, none of them a release-path
  input (`release.yaml`, `packaging/extensions/`, `examples/`, `src/engine.rs`, `Cargo.toml`,
  `Cargo.lock`, `rust-toolchain.toml`). The four-leg CI evidence therefore still describes the
  release path at HEAD.
- F1's negative control re-run independently. The `neg-view` tarball's `bin/xdu-view` fails to exec
  with "bad interpreter", rc 127. `check.sh` on that tarball still prints `extension-check: ok
  platform=osx_arm64 engine=v1.4.4`, rc 0.

Not observed:

- The Dockerfile image build at HEAD. No container runtime here; the reviewer graded it by reading.
  A CI run on 9cd2582 was in progress at review time and is reported separately.
- The Test workflow (Lint, Test, Packaging) at HEAD. The evidence file records it green at ab0f7d9;
  the P4 commits since touch `install.sh`, `Dockerfile`, `hpccm/` and docs.
- The man-page gate. The diff touches no `doc/*.scd`, and `scdoc` is not installed on this host, so
  no page was rendered and no literal was confirmed by any form. The gate's state on HEAD and on
  `main` is unobserved, not satisfied.
- The `linux_amd64`, `linux_arm64` and `osx_amd64` legs locally. Their CI outcome is in the evidence
  file; the underlying logs were not re-fetched in this session.

## Requirement → evidence matrix

| R-ID | Implemented by | Verified how (and by whom) | Status |
|------|----------------|----------------------------|--------|
| R1 | `src/engine.rs` `PINNED_EXTENSIONS`; `examples/extension-probe.rs` manifest; `packaging/extensions/{fetch,footer,check}.sh`; `release.yaml` (one build invocation, fetch, `tar … bin share lib`, check before upload, publish skipped on PR) | Reviewer: local `osx_arm64` leg, plus footer, check and fetch negative controls refusing a wrong platform, a wrong engine, an extra or missing file, and a bad manifest. Orchestrator: `verify/p3.sh` and the four-leg CI evidence, all `engine=v1.4.4`. | ✅ |
| R2 | `AGENTS.md`, `invariants.md` §13, `install.sh`, `hpccm/xdu.py`, `Dockerfile`, `.dockerignore` | Reviewer: `install.sh` drives (plain, symlinked, re-install), HPCCM specs regenerated byte-identical, Dockerfile read. Orchestrator: Dockerfile CI at HEAD not observed. | ✅ (Dockerfile by reading) |
| R3 | `engine::open_connection`, `extension_dir`, `load_extension_from`; `clippy.toml` | Reviewer: override, symlink and empty-override drives; autoload on/off control; clippy negative control; cold `HOME` empty throughout. | ✅ |
| R4 | `engine::load_extension_from` (file check before any SQL) | Reviewer: a probe with no `lib/` and a missing override both fail naming the full path and "does not download"; `check.sh`'s three absent cases; unit tests. | ✅ |
| R5 | `xdu-find`, `xdu-view`, `xdu-rm` open through `engine::open_connection`; none loads an extension | Reviewer: `offline_tests` 1/1; throwaway-index drives under a cold `HOME`; reader SQL functions on a refusing connection. | ✅ |
| R6 | `release.yaml` (`--bins --example extension-probe` in one invocation; `check.sh` on the exact tarball before upload); `check.sh` | (a) Reviewer: local `osx_arm64` leg loads `httpfs` from the extracted layout. Orchestrator: four-leg CI evidence. (b) Reviewer and orchestrator: `bin/xdu-view` is never executed (F1). | ⚠️ (a) met, (b) partial |

Unmapped changes:

- `test.yaml` moves the macOS test runner from `macos-14` to `macos-15`.
- `release.yaml` moves the `aarch64-apple-darwin` leg from `macos-14` to `macos-15`. The
  `x86_64-apple-darwin` move to `macos-15-intel` does map to R6, so the probe runs natively.
- The new `pull_request` trigger on `release.yaml` is required by no R-ID, but it produces the R1
  and R6 evidence. It holds read-only permissions, reads no secrets, and skips publish.
- Deferral records with no code change, filed per `AGENTS.md`'s deferral rules:
  - `issues/hpccm-extension-guard.md` and `issues/macos-sdk27-bundled-duckdb-build.md`, each with a
    ROADMAP entry
  - a new entry in `issues/native-os-packages-deb-rpm.md`
  - hand-off notes in `issues/s3-index-target.md`
  - the status line in `issues/duckdb-extension-distribution.md`

None of these blocks the cycle.

## Findings

### [MEDIUM/CONFIRMED] F1: the release check never runs `bin/xdu-view`
- **Where:** `packaging/extensions/check.sh:49` and `:100-102`. The cold-`HOME` tool runs are at
  `:104-113`.
- **Failure scenario:** `check.sh` executes `xdu`, `xdu-find` and `xdu-rm` from the extracted
  tarball. For `xdu-view` it only tests `-x` and byte-greps for the engine `source_id`. A tarball
  whose `xdu-view` cannot exec passes the leg and publishes, for example because of a loader,
  glibc or libstdc++ floor breach. That is the inferred-from-symbols gap R6 exists to close.
- **Evidence:** The negative-control tarball is identical to a passing `osx_arm64` one, except
  `bin/xdu-view` is `#!/nonexistent/interpreter` plus the `source_id` string. Run directly, it
  fails with "bad interpreter", rc 127. `sh packaging/extensions/check.sh neg-view.tar.gz
  extension-probe` prints `extension-check: ok platform=osx_arm64 engine=v1.4.4`, rc 0. Both the
  reviewer and the orchestrator reproduced it. The real `xdu-view --version` exits 0 with no TTY
  and a cold `HOME`, so it can be run here.
- **Touches invariant / requirement:** R6(b).

### [MEDIUM/CONFIRMED] F2: `AGENTS.md`'s Repository map omits the modules and files this diff adds
- **Where:** `AGENTS.md:292-316`.
- **Failure scenario:** The map lists every `src/` module except the new `src/engine.rs`, which is
  now the only place a DuckDB connection may be opened. Neither `AGENTS.md` nor `invariants.md`
  names `examples/extension-probe.rs` or `clippy.toml`, whose `disallowed-methods` rule enforces
  the refusal. `packaging/extensions/` is named once, at `AGENTS.md:541`, and not in the map. An
  agent working from the map does not learn the connection rule or the probe's role in the release
  gate.
- **Evidence:** The reviewer grepped `AGENTS.md` and `.agents/factory/invariants.md` for
  `engine.rs`, `src/engine.rs`, `examples/extension-probe.rs`, `extension-probe`, `clippy.toml`,
  `disallowed` and `PINNED_EXTENSIONS`, with zero hits for each. `engine::open_connection` appears
  once, at `AGENTS.md:504`. The orchestrator re-ran the grep with the same result. `invariants.md`
  §13's layout bullet is accurate against the code, so no gate list is stale.
- **Touches invariant / requirement:** rubric scope 5 (operating-manual drift).

### [LOW/CONFIRMED] F3: the probe fails a correct load when `XDU_EXTENSION_DIR` is relative
- **Where:** `examples/extension-probe.rs:130` and `:139`.
- **Failure scenario:** `engine::extension_dir()` returns an override as given, for example `rel`.
  `load_extension_from` returns the absolutized path, so `loaded.starts_with(&dir)` is false. The
  library loads the extension correctly; only the probe's own assertion fails. No release leg is
  affected, because `check.sh` runs the probe under `env -i` with absolute overrides only.
- **Evidence:** `(cd $r/cwd && env -i PATH=$PATH HOME=$r/home XDU_EXTENSION_DIR=rel
  $r/nolib/bin/extension-probe load)` prints `extension-probe: httpfs loaded from
  …/cwd/rel/httpfs.duckdb_extension, outside rel`, rc 1.
- **Touches invariant / requirement:** R3 (override honoured). The defect is in the probe, not the
  loader.

## Human-gate triggers

- Not triggered. F1 is in `packaging/extensions/check.sh`, F2 in `AGENTS.md`'s map, F3 in
  `examples/`. None is a high-blast-radius file or touches `invariants.md` §1, §2, §4 or §5. The
  diff's touches to `src/bin/xdu-rm.rs` and `src/lib.rs` only swap the connection constructor and
  declare the module. The reviewer drove `xdu-rm`'s dry-run and real run with `--limit` and they
  selected identical rows.
- For the publish gate: the R6 amendment (f3f09c2) was made mid-cycle at the PLAN gate.

## Review cycle 2 — changes-requested (2026-11-05)

- **Reviewed commit:** b4d8d9f  ·  **Base:** main (b47ba2b)  ·  **Date:** 2026-11-05
- **Verdict:** changes-requested
- **Cycle:** 2 of ≤3, mirroring `review.cycle` in `TECH.md`.

**Mode.** A fresh blind pass over the full spec-excluded diff, which is the default for a later
cycle, rather than a pass scoped to the cycle-1 fixes. The reviewer was not told about cycle 1's
findings. The orchestrator checked the F1–F3 remediation separately (below). No `debate` or
`completeness` argument was passed.

**Contract drift.** `GOAL.md` has the same three commits as at cycle 1 (1615832, 8f7e96c,
f3f09c2). The gate judge confirmed the R6 amendment (f3f09c2) at cycle-1 triage, under the
maintainer's standing mandate. This cycle grades the amended R6.

**Head.** b4d8d9f changes only `spec/` files relative to 78daf41. The reviewer confirmed that
`git diff 78daf41 HEAD -- . ':(exclude)spec/'` is empty. The release run at 78daf41 therefore
built the exact non-spec tree under review.

### Verification run

Reviewer (blind), on this macOS arm64 host:

- `cargo fmt --all -- --check` → clean.
- `cargo clippy --offline --locked --all-targets --all-features -- -D warnings` → clean.
- `cargo test --offline --locked` → all pass:
  - lib 101 (11 in `engine::tests`), xdu-view 5, cli_version 1, crawl 25, find 9, offline 1,
    rm 18, version 6
  - the probe's 4 unit tests, which now run under `cargo test`
  - `non_utf8_directory_is_refused_before_any_sql` skips its body here, because the filesystem
    rejects a non-UTF-8 name. It runs for real only on Linux.
- Host `osx_arm64` leg, run end to end the way `release.yaml` runs it:
  - one `cargo build --release --locked --target aarch64-apple-darwin --bins --example
    extension-probe`
  - `extension-probe manifest` → `engine=v1.4.4 platform=osx_arm64 source_id=6ddac802ff
    extension=httpfs`
  - `fetch.sh` → `footer: ok … sha256=9a3eef9c…`, the same hash CI's `osx_arm64` leg logged
  - `tar … bin share lib`, with real completions and stub man pages because `scdoc` is absent
  - `check.sh` → `extension-check: ok platform=osx_arm64 engine=v1.4.4`, rc 0
- `check.sh` negative controls, all on copies, all rc 1. Each failed at the intended check:
  - wrong platform; wrong engine
  - an extra extension, a hidden stray file, a subdirectory, a missing extension, no `lib/`
  - one byte flipped with the footer intact, which passed the footer check and then failed at
    `LOAD`, so the signature check really runs
  - `bin/xdu-view` not executable; a dead or wrong-version `xdu-view`
  - the crawler copied in as `xdu-rm`; a dead `xdu`; an `xdu` printing a malformed version
  - a dead `xdu-find`; an `xdu-find` miscounting
  - a tool that writes under the cold `HOME`
  - Exporting `XDU_EXTENSION_DIR` around the script did not change the verdict, so `env -i`
    isolates it.
- `fetch.sh` refused each bad fake-probe manifest:
  - a failing probe, a duplicate `engine=`, an unknown platform, `extension=../evil`
  - no extensions, an unshaped engine string, an unpublished engine version
- `footer.sh` refused each malformed input: a short file, random bytes, a directory, the wrong
  platform, the wrong engine, the wrong argument count, and an empty file.
- Extension-directory drives with `extension-probe load` under a cold `HOME`, all as specified:
  - the default directory
  - an absolute, a relative, an empty and a missing override, and an override naming an empty
    directory
  - a symlinked executable, and invocation by relative path and through `PATH`
  - the cold `HOME` stayed empty throughout
  - Control: a plain `open_in_memory()` reports autoinstall and autoload on. It reaches for
    `extensions.duckdb.org` and writes `.duckdb/extensions/v1.4.4/osx_arm64` into the cold `HOME`.
  - Clippy control: a scratch crate calling `open_in_memory` under the repo's `clippy.toml` fails
    with `disallowed_methods`.
- `temp_index.sh` drives under a cold `HOME`:
  - `xdu-find --count`, `-u alice` and a glob pattern all behaved
  - `xdu-rm --dry-run --limit 2` selected the same two files that `--force --limit 2` then deleted
  - `xdu-view`'s SQL shapes run on a refusing connection, which keeps only `core_functions` and
    `parquet` loaded
  - The TUI itself could not be driven, because the sandbox denies `openpty`.
- `install.sh` with a local `curl` stand-in and no network:
  - a plain prefix and a re-install, which leaves no temp files behind
  - a symlinked install directory: the extension lands under the physical path, and a probe
    reached through the symlink loads it
  - `XDU_INSTALL` set apart from `XDU_PREFIX`
  - an old tarball with no `lib/`, which creates no `lib`
  - These drives needed the harness-only `mktemp` stand-in, which is the environment, not the
    product.
- HPCCM: specs regenerated with `uvx --from hpccm==26.5.0` into `target/` → `cmp`-identical to the
  committed `xdu.def` and `xdu.docker`.
- Grep of the non-spec diff for spec `R#`/`P#` ids in source: none. The only hits are the existing
  `Found by:` convention in `issues/*.md`.

Orchestrator:

- **CI release run at 78daf41**, from its full log:
  - Every job checked out `78daf41`, and `TAG` was `pr31`.
  - Each of the four legs ends its check step on `extension-check: ok`, with
    `platform={linux_amd64,linux_arm64,osx_arm64,osx_amd64}`, all `engine=v1.4.4`.
  - Both Linux legs passed "Assert glibc floor", with all four binaries at most `GLIBC_2.28`.
  - `linux_amd64` printed `boundary=ok IO Error: …`, with no abort.
  - The log has no `##[error]` line and no publish job log, so publish was skipped on the pull
    request.
  - `evidence/p3-pr-run.md` quotes these lines verbatim and names this run and head.
- `sh spec/duckdb-extension-distribution/verify/p3.sh` → `P3-GATE-GREEN`.
- Host leg re-run independently from a snapshot of the HEAD build: `fetch.sh` (same sha256), the
  tarball, then `check.sh` → `extension-check: ok platform=osx_arm64 engine=v1.4.4`.
- A path containing `'` in `XDU_EXTENSION_DIR` is refused before any SQL, rc 1, with the cold
  `HOME` left empty (finding F4).

Cycle-1 remediation, checked by the orchestrator:

- **F1 fixed.** `check.sh` now runs `--version` for all four shipped binaries under the cold
  `HOME`, stdin `/dev/null`, and matches `<name> X.Y.Z`. The cycle-1 control was re-run: a
  `bin/xdu-view` that is `#!/nonexistent/interpreter` and carries the `source_id`. It now fails
  with `check.sh: bin/xdu-view --version did not run: …`, rc 1; at cycle 1 it passed, rc 0. The
  loop sits before the `ok` line under `set -eu`, so every CI leg's `ok` line implies all four ran.
- **F2 fixed.** `AGENTS.md`'s map now lists `src/engine.rs`, `examples/extension-probe.rs`,
  `clippy.toml` and `packaging/extensions/`. Each entry matches the code: the public API of
  `engine.rs` and the four methods `clippy.toml` disallows.
- **F3 fixed.** The probe compares against the absolutized directory:
  - `XDU_EXTENSION_DIR=rel` and `=./rel` both load, rc 0.
  - A missing relative override fails naming `rel2/httpfs.duckdb_extension`, rc 1.
  - Four unit tests cover the comparison, including a sibling directory that shares a name
    prefix.

Not observed:

- The Dockerfile image build. There is no container runtime here, so it was graded by reading.
  The Docker "Validate build" jobs at 78daf41 were reported green with the run; their logs were
  not read in this session.
- The Test workflow (Lint, `ubuntu-24.04`, `macos-15`, Packaging) at 78daf41. It was reported
  green with the run; its log, including the man-page literal assertions, was not read here.
- The man-page gate locally. `scdoc` is not installed, so no page was rendered and no literal was
  confirmed by any form. The diff touches no `doc/*.scd` and no `src/cli.rs`, so no flag set changed
  to compare against. The release run's Prepare job rendered every page, but that shows exit 0
  only.
- The `linux_amd64`, `linux_arm64` and `osx_amd64` legs locally. They are graded from the CI log.

### Requirement → evidence matrix

| R-ID | Implemented by | Verified how (and by whom) | Status |
|------|----------------|----------------------------|--------|
| R1 | `engine::PINNED_EXTENSIONS` (a list); probe `manifest`; `fetch.sh`, `footer.sh`; `check.sh` exact-set, footer and `source_id` checks; `release.yaml` fetch → `tar … bin share lib` → check before upload; publish needs both build jobs and skips pull requests | Reviewer: host leg, plus platform, engine, set and signature negative controls. Orchestrator: four-leg CI log at 78daf41, all `engine=v1.4.4`, and the evidence file. | ✅ |
| R2 | `AGENTS.md` layout contract, `invariants.md` §13, `install.sh`, `hpccm/xdu.py`, `Dockerfile`, `.dockerignore` | Reviewer: `install.sh` drives (plain, re-install, symlinked, split prefix, old tarball); HPCCM specs regenerated `cmp`-identical; Dockerfile by reading. Orchestrator: Docker CI reported, not read. | ✅ (Dockerfile by reading) |
| R3 | `engine::open_connection`, `extension_dir`, `load_extension_from`; `clippy.toml` | Reviewer: override, relative and symlink drives under a cold `HOME`; autoload on/off control; clippy control. Orchestrator: relative-override drives. | ✅ |
| R4 | `engine::load_extension_from` absent-file branch | Reviewer: missing and empty overrides and no `lib/` fail naming the path and "does not download"; the missing directory is not created. Orchestrator: missing relative override. | ✅ |
| R5 | readers open through `engine::open_connection`; none loads an extension | Reviewer: `offline_tests` 1/1; throwaway-index drives under a cold `HOME`; reader SQL on a refusing connection. | ✅ |
| R6 | `release.yaml`, one build invocation for bins and probe; `check.sh` on the exact tarball: probe load from the extracted layout, `--version` of all four binaries, and `xdu`/`xdu-find`/`xdu-rm` drives, all under `env -i HOME=<cold>` | (a) and (b): Reviewer, host leg and exec negative controls. Orchestrator: four-leg CI log; the cycle-1 F1 control now fails. | ✅ |

Unmapped changes. None of these blocks the cycle.

- The cycle-1 items carry over:
  - `test.yaml` and the `aarch64-apple-darwin` release leg move to `macos-15`.
  - `release.yaml` gains a read-only `pull_request` trigger.
  - The deferral records in `ROADMAP.md` and `issues/`.
- New this cycle:
  - The `AGENTS.md` map gains a `packaging/rpm/` line for a directory that already exists on
    `main`.
  - `README.md` describes `XDU_EXTENSION_DIR` and `cargo install` builds. They are operator-facing,
    although the loader is library-only this cycle.

### Findings

#### [HIGH/CONFIRMED] F4: the DuckDB injection invariant does not name the extension path this diff sends to SQL
- **Where:** `.agents/factory/invariants.md` §5 and `AGENTS.md` load-bearing invariant 5
  (`AGENTS.md:461-463`). The new surface is `src/engine.rs:139`, `LOAD '{literal}'`.
- **Failure scenario:** The diff adds a new operator-controlled value that reaches SQL. The
  extension path comes from `XDU_EXTENSION_DIR` or from the canonical executable path, and is
  interpolated into `LOAD`. Both invariant texts still bound the surface to `read_parquet(...)` and
  `WHERE`/`LIKE`, list `--pattern`, `--partition` and the index path, and §5 is headed "(all
  readers)". A later cycle that walks §5 to decide whether its change touches the injection surface
  is not pointed at `engine.rs`. The S3 cycle will extend that module. A regression there would then
  not be read as a §5 violation, so it would get no auto-CRITICAL and no human gate. The code is
  safe today: `load_extension_from` refuses a `'` or non-UTF-8 path before any SQL, and a unit test
  covers it.
- **Evidence:**
  - `grep -nE 'LOAD|XDU_EXTENSION_DIR|engine' .agents/factory/invariants.md` matches only §13's
    layout bullet (line 244), nothing in §5.
  - `AGENTS.md:461` reads "Every user value reaching `read_parquet(...)`/`WHERE` is an injection
    surface."
  - `git grep -nE 'format!\(.*(LOAD|SET|INSTALL|PRAGMA)' -- src examples` finds the `LOAD` at
    `engine.rs:139`, plus a `SET` inside the test module.
  - Running `extension-probe load` with `XDU_EXTENSION_DIR=…/it's` prints "refusing to load DuckDB
    extension from a path containing a single quote", rc 1. The env var does reach that guard.
  - Reproduced by both the reviewer and the orchestrator.
- **Severity:** The reviewer proposed MEDIUM, noting that a strict reading gives HIGH. The rubric
  rates as HIGH any drift that degrades a gate through text in `invariants.md` or an `AGENTS.md`
  load-bearing invariant, which is where this sits. §5 also decides the human gate, so HIGH stands.
- **Touches invariant / requirement:** rubric scope 5, operating-manual drift, against §5. It is
  not a §5 violation in code.
- **Remediation:** documentation only. In one commit, keeping the two files in lockstep, amend §5
  and `AGENTS.md` invariant 5 so the surface includes the extension path that reaches `LOAD`. Name
  its sources (`XDU_EXTENSION_DIR`, the canonical executable path) and the rule held at
  `engine::load_extension_from`: refuse `'` and non-UTF-8 before any SQL. No release-path input
  changes, so the 78daf41 CI evidence stays valid.

No other candidate survived refutation. The reviewer dropped four by executed check:

- `install.sh` with a root-level `XDU_INSTALL`
- HPCCM with an empty prefix
- `docker.yaml`'s path list without `src/engine.rs`, which the release pull-request legs cover
- the probe panicking on a closed pipe, which is release-time only

### Human-gate triggers

- **Triggered, on a literal reading.** F4 is CONFIRMED and concerns the §5 SQL-injection
  invariant, one of the four that always require sign-off. No high-blast-radius file is
  implicated. The diff changes `src/bin/xdu-rm.rs` and `src/lib.rs` only by swapping the
  connection constructor and declaring the module. The reviewer drove `xdu-rm` with `--dry-run`
  and then with `--force`, both with `--limit`, and the two runs selected identical rows. The
  sign-off is for the §5 text amendment; the code path is safe as built.
- Cycle 2 of ≤3. A third cycle is the last before escalation.
