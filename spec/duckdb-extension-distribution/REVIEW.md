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
