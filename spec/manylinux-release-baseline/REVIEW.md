# REVIEW — Portable Linux release baseline for RHEL8/9

> Adversarial QA by `xdu-review`, run in an isolated/clean context. The correctness pass grades the
> branch diff against [`GOAL.md`](GOAL.md) + the AGENTS.md invariants **only** — it does not see
> `PLAN.md`/`TECH.md` (avoids grading-its-own-homework / plan-sycophancy). Every finding cites an
> **executed** command, not an assertion.

- **Reviewed commit:** 366916d79de2c2fecadfae5e75af9916bd373a7e  ·  **Base:** main  ·  **Date:** 2026-09-12
- **Verdict:** changes-requested
- **Cycle:** 1 of ≤3 — mirrors `review.cycle` in `TECH.md` (escalate to human on non-convergence)

## Verification run

Commands actually executed and their outcomes (the spine of the review). The blind reviewer ran all
of these; the orchestrator spot-reproduced F1 and the tree checks.

- Reviewer: parsed `release.yaml` with pyyaml — `build` matrix resolves exactly to the two
  `manylinux_2_28` images; guard step present on both gnu legs with no `if:`, ordered Build →
  Assert → Download → Assemble → Upload → green.
- Reviewer: `docker run` against both builder images — `ldd (GNU libc) 2.28` on each; `docker
  manifest inspect` OK for both; `objdump`+`curl` present, no `cargo` (matches the rustup step).
- Reviewer: 9-case simulation of the verbatim guard loop with a stub `objdump` — pass(2.28)→0,
  breach(2.38)→1, empty→1, `GLIBCXX_3.4.29`→1, `GCC_12.0.0`→1, absent-C++→0, surprise
  `libssl.so.3`→1, plus `CASE private EXIT 0` (the F1 defect below).
- Reviewer: JSON/stanza diff of macOS entries and assemble/upload/download/checkout bodies vs `git
  show main:...` — byte-identical; `publish` identical except `needs: [build, build-macos]`.
- Reviewer: regenerated both hpccm specs with hpccm 26.5.0 (`uvx --from hpccm`, docker and
  singularity formats) — both byte-identical to the committed specs.
- Orchestrator: `echo 'DF *UND* GLIBC_PRIVATE foo' | grep -o 'GLIBC_[0-9.]*'` emits `GLIBC_`
  (non-empty, bypasses the empty-guard) — F1 reproduced; the tightened pattern `GLIBC_[0-9][0-9.]*`
  emits nothing there and still matches `GLIBC_2.28`.
- Orchestrator: `cargo fmt --all -- --check` → clean; `git status --porcelain` → empty on hand-back;
  `git log main..HEAD -- spec/manylinux-release-baseline/GOAL.md` shows only the shaping commit (no
  contract drift). No `src/` change, so the Rust suite is out of scope for this diff.
- Not executed (stated as gaps, not defects): R1 end-to-end on RHEL8 (no binaries ship pre-release),
  a cold bundled-DuckDB build inside manylinux (fail-closed if it breaks — red leg, no tarball), and
  the post-release recipe smoke at a new tag (no such tag exists yet).

## Requirement → evidence matrix

Bidirectional traceability. No R-ID lacks an implementing change; unmapped changes are listed below.

| R-ID | Implemented by (file/commit) | Verified how | Status |
|------|------------------------------|--------------|--------|
| R1 | `release.yaml` builder floor + guard-before-assembly; recipe `--version` smoke retained (`hpccm/xdu.py`) | Builder glibc proven 2.28 on both arches via `docker run`; delivery chain inspected; end-to-end run impossible pre-release (recorded gap) | ✅ structurally |
| R2 | `release.yaml:90-104` (`container.image: ${{ matrix.image }}`, both `manylinux_2_28` legs) | YAML parse + `ldd` 2.28 on both images | ✅ |
| R3 | `release.yaml:140-166` (`Assert glibc floor`, pre-assembly, fail-closed) | 9-case loop simulation incl. breach/empty/C++-ceiling/allowlist cases | ✅ modulo F1 |
| R4 | `release.yaml` `build-macos` split; assemble/upload duplicated | Byte-identity diff vs `main` for macOS entries and all shared stanzas | ✅ |
| R5 | `hpccm/xdu.py` default `runtime_base` → bookworm-slim; `xdu.def`/`xdu.docker` regen | hpccm regen byte-identical; `VERSION '0.5.1'` == `Cargo.toml 0.5.1`; `ubuntu-24.04-arm` precedent in `docker.yaml:63,99` | ✅ |

Unmapped changes (possible scope creep): `issues/manylinux-release-baseline.md` status flip (benign
factory bookkeeping); macOS build step dropping the triple-scoped `CC/CXX_aarch64_*` env (benign —
dead vars for apple targets); `hpccm/xdu.py` usage comment `> Dockerfile` → `> xdu.docker` (benign
and accurate per `hpccm/Makefile:89`). Non-goals confirmed clean: zero hunks under `src/`,
`doc/*.scd`, `Cargo.toml`/`Cargo.lock`, top-level `Dockerfile`, `install.sh`; no `R#`/`P#` ids in the
non-spec diff.

## Findings

Severity: **CRITICAL** (any `invariants.md` §1–§12 violation is auto-CRITICAL, **including lettered
subsections** such as §2b/§2c; a §13 project-conventions violation is **HIGH**) · **HIGH** · **MEDIUM** · **LOW**. Verdict: **CONFIRMED**
(reproduced) vs **PLAUSIBLE** (suspected, needs human triage). Only CONFIRMED findings auto-loop to
`xdu-build`.

### [LOW/CONFIRMED] Floor guard vacuously passes on `GLIBC_PRIVATE`-only output
- **Where:** `.github/workflows/release.yaml:146` (same shape at `:156` for the C++ families)
- **Failure scenario:** a binary whose `objdump -T` output mentions `GLIBC_PRIVATE` but carries no
  versioned `GLIBC_X.Y` node yields `max="GLIBC_"` — `[0-9.]*` matches empty — which is non-empty,
  so it sails past the `-z` guard at lines 147–149, and `sort -Vu` ranks the bare prefix below
  `GLIBC_2.28`, so the leg passes. The step's own comment promises the opposite: an empty maximum
  fails rather than passing vacuously.
- **Evidence:** verbatim-loop simulation with a stub `objdump`: `CASE private EXIT 0` against `CASE
  empty EXIT 1`; orchestrator spot-check `echo 'DF *UND* GLIBC_PRIVATE foo' | grep -o
  'GLIBC_[0-9.]*'` prints `GLIBC_`.
- **Touches invariant / requirement:** R3 (guard fail-closed posture). Unreachable with the real
  toolchain — any dynamically linked glibc binary carries versioned nodes, so the degenerate token
  never decides a real leg — hence LOW, not HIGH. Fix is one line: require a leading digit,
  `GLIBC_[0-9][0-9.]*` (and the same for `GLIBCXX_`/`GCC_`, whose `*` has the identical shape), then
  re-run the simulation. Dropped as non-findings after refutation: `set -e`/pipefail aborts,
  comparison direction, `GCC_` cross-matching `GLIBCXX_*`, allowlist shape, `sort -V` ordering.

## Human-gate triggers

Set if any CONFIRMED finding touches the high-blast-radius core (`src/bin/xdu-rm.rs`,
`src/bin/xdu.rs`, `src/crawl.rs`, `src/lib.rs`, `src/cli.rs`) or a destructive-rm / schema-stability /
atomic-write / SQL-injection invariant — these **always** require human sign-off before
`xdu-publish`, regardless of auto-loop. (`invariants.md`'s *High-blast-radius files* header is the
authoritative path list; this copy may only ever **widen** to match it.)

- Not triggered. F1 touches only `.github/workflows/release.yaml` and no §1/§2/§4/§5 invariant, so
  the fix loops to `/xdu-build` without a human gate.

## Optional completeness sub-pass (separate reviewer; may see TECH.md)

- Not run — plain `/xdu-review`, no `completeness` argument.

## Review cycle 2 — approved (2026-09-13)

Mode: fresh blind pass over the full spec-excluded diff (`git diff main...HEAD -- .
':(exclude)spec/'`), head `1ffa9f4b8fd28bd6fdee37f8cfaf38255498ef2c`, base `main`.
Prior cycle 1 verdict was changes-requested on one LOW/CONFIRMED (F1 guard vacuous-pass);
this cycle grades the F1 fix plus regressions. No `completeness` sub-pass (no argument).
No contract drift: `git log main..HEAD -- spec/manylinux-release-baseline/GOAL.md`
shows only the shaping commit `01cecf5`.

### Verification run

- Reviewer (blind): Ruby YAML parse — `build` matrix resolves exactly to the two
  `manylinux_2_28` images via `container.image: ${{ matrix.image }}`; `Assert glibc
  floor` ordered Build → Assert → Download → Assemble → Upload; `docker manifest
  inspect` OK for both quay tags; aarch64 image executed → `ldd (GNU libc) 2.28`;
  guard dependencies (`objdump` GNU 2.41, `curl`, `gcc`/`g++` 14.2.1, `sort`, `awk`)
  resolved via executed `docker run`.
- Reviewer (blind): full-harness guard simulation under `set -eu` — GLIBC 2.17/2.28 →
  PASS, 2.29/2.34/2.39 → BREACH, empty → fail-loud, PRIVATE-only → empty → fail-loud
  (F1 closed); GLIBCXX 3.4.22/3.4.25 → PASS, 3.4.29 → BREACH; GCC 8.0.0 → PASS,
  12.0.0 → BREACH; absent C++ need → PASS; NEEDED allowlist passes the six known
  libs + `ld-linux*` and catches synthetic `libcurl.so.4`; terminal
  `[ "$fail" -eq 0 ]` propagates correctly. Class sweep: no remaining `_[0-9.]*`
  loose pattern.
- Reviewer (blind): macOS entries and assemble/upload/download/checkout bodies
  byte-identical vs `git show main:...`; only removed steps are the gnu-aarch64
  cross toolchain belonging to the moved leg; `publish` identical except
  `needs: [build, build-macos]`.
- Reviewer (blind): regenerated both hpccm specs with hpccm 26.5.0 — byte-identical
  to committed `xdu.def`/`xdu.docker`; only `trixie` remnant is the intentional
  historical sentence in `hpccm/README.md:56`.
- Orchestrator: `git status --porcelain` → empty on hand-back; tight pattern
  confirmed at `release.yaml:146` and `:156`
  (`GLIBC_[0-9][0-9.]*`, `${famname}_[0-9][0-9.]*`); loose `_[0-9.]*` absent;
  `printf 'DF *UND* GLIBC_PRIVATE foo' | grep -o 'GLIBC_[0-9][0-9.]*'` → empty
  (`TIGHT-EMPTY-OK`) while `GLIBC_2.28` still matches; `cargo fmt --all -- --check`
  → clean; `git diff main...HEAD -- src/` → empty.
- Not observed (not defects): `cargo test`, `cargo clippy` — no `src/` change, so
  the Rust suite is out of scope for this diff; CI rollup state — this session has
  no `gh`, per skill; R1 end-to-end on RHEL8, cold bundled-DuckDB manylinux build,
  post-release recipe smoke — impossible pre-release, fail-closed (red leg blocks
  `publish`).

### Requirement → evidence matrix (who verified each)

| R-ID | Implemented by | Verified how (owner) | Status |
|------|----------------|----------------------|--------|
| R1 | `release.yaml` builder floor + guard-before-assembly; recipe `--version` smoke retained | Reviewer: builder `ldd` 2.28 + chain inspection; end-to-end impossible pre-release (recorded gap) | ✅ structurally |
| R2 | `release.yaml` manylinux_2_28 legs | Reviewer: YAML parse + `ldd` 2.28 both arches + manifest | ✅ |
| R3 | `release.yaml:140-166` Assert floor, pre-assembly fail-closed | Reviewer: full simulation incl. F1 PRIVATE case; Orchestrator: tight-pattern spot-check | ✅ (F1 closed) |
| R4 | `build-macos` split; assemble/upload duplicated | Reviewer: byte-identity diff vs `main` | ✅ |
| R5 | `hpccm/xdu.py` default → bookworm-slim; specs regen | Reviewer: hpccm 26.5.0 regen byte-identical | ✅ |

Unmapped changes: `issues/manylinux-release-baseline.md` status flip (factory
bookkeeping); `hpccm/xdu.py:5` usage comment `> Dockerfile` → `> xdu.docker`
(accurate per `hpccm/Makefile:89`); macOS step dropping dead `CC/CXX_aarch64_*`
env. All benign, none block. Non-goals clean: zero hunks under `src/`,
`doc/*.scd`, `Cargo.toml`/`Cargo.lock`, top-level `Dockerfile`, `install.sh`; no
`R#`/`P#` ids in the non-spec diff (reviewer grep clean).

### Findings

No CONFIRMED or PLAUSIBLE findings. Prior F1 LOW/CONFIRMED verified fixed and not
re-reported: tightened patterns landed at both extraction sites, PRIVATE-only input
now extracts empty and fails loud, `GLIBC_2.28` still matches.

### Human-gate triggers

Not triggered. No CONFIRMED finding touches the high-blast-radius core
(`src/bin/xdu-rm.rs`, `src/bin/xdu.rs`, `src/crawl.rs`, `src/lib.rs`,
`src/cli.rs`) or a §1/§2/§4/§5 invariant. No `src/` change at all. Proceed to
`/xdu-publish` without mandatory sign-off.
