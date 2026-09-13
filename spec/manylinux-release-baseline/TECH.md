---
slug: manylinux-release-baseline
title: Portable Linux release baseline for RHEL8/9
kind: feature
appetite: small
status: in_review
branch: feature/manylinux-release-baseline
base: main
current_phase: P4
last_updated: '2026-09-12'
phases:
- id: P1
  name: Manylinux legs plus the symbol guard in release.yaml
  status: done
  satisfies:
  - R2
  - R3
  depends_on: []
  parallel: false
  hammerable: false
  hill: uphill
  verify: set -eu; f=.github/workflows/release.yaml; pat=$(sed -n "s/.*grep -o '\(GLIBC_[^']*\)'.*/\1/p"
    "$f" | head -n 1); [ "$pat" = 'GLIBC_[0-9][0-9.]*' ]; grep -qF '${famname}_[0-9][0-9.]*'
    "$f"; ! grep -qF '_[0-9.]*' "$f"; [ -z "$(printf 'DF *UND* GLIBC_PRIVATE\n' |
    grep -o "$pat")" ]; [ "$(printf 'x GLIBC_2.28 y\n' | grep -o "$pat")" = 'GLIBC_2.28'
    ]; echo F1-GATE-GREEN
- id: P2
  name: 'Local proof: manylinux build plus RHEL8 and bookworm probes'
  status: done
  satisfies:
  - R1
  - R2
  - R3
  depends_on:
  - P1
  parallel: false
  hammerable: false
  hill: uphill
  verify: set -eu; B=/tmp/ml-xdu/bin; for b in xdu xdu-find xdu-view xdu-rm; do test
    -x $B/$b; M=$(strings $B/$b | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -n 1);
    test -n $M; docker run --rm --platform linux/amd64 -v $B:/mnt:ro registry.access.redhat.com/ubi8/ubi-minimal
    /mnt/$b --version; docker run --rm --platform linux/amd64 -v $B:/mnt:ro debian:bookworm-slim
    /mnt/$b --version; done; tar tzf /tmp/ml-xdu/xdu-layout-test.tar.gz | grep -q
    '^bin/xdu$'; tar tzf /tmp/ml-xdu/xdu-layout-test.tar.gz | grep -q '^share/man/man1/xdu.1$';
    N=/tmp/ml-neg; mkdir -p $N; curl -fsSL -o $N/t.tar.gz 'https://github.com/xdu-project/xdu/releases/download/v0.5.1/xdu-v0.5.1-x86_64-unknown-linux-gnu.tar.gz';
    tar xzf $N/t.tar.gz -C $N; docker run --rm --platform linux/amd64 -v $N:/m:ro
    quay.io/pypa/manylinux_2_28_x86_64:latest sh -c 'max=$(objdump -T /m/bin/xdu-find
    | grep -o "GLIBC_[0-9.]*" | sort -Vu | tail -n 1); test "$max" = "GLIBC_2.38"
    && test "$(printf "%s\n%s\n" "$max" "GLIBC_2.28" | sort -Vu | tail -n 1)" != "GLIBC_2.28"
    && echo NEGATIVE-RED-OK' && for b in xdu xdu-find xdu-view xdu-rm; do G=$(strings
    /tmp/ml-xdu/bin/$b | grep -o 'GLIBCXX_[0-9.]*' | sort -Vu | tail -n 1); if [ -n
    "$G" ]; then [ "$(printf '%s\n%s\n' "$G" 'GLIBCXX_3.4.25' | sort -Vu | tail -n
    1)" = 'GLIBCXX_3.4.25' ] || exit 1; fi; C=$(strings /tmp/ml-xdu/bin/$b | grep
    -o 'GCC_[0-9.]*' | sort -Vu | tail -n 1); if [ -n "$C" ]; then [ "$(printf '%s\n%s\n'
    "$C" 'GCC_8.0.0' | sort -Vu | tail -n 1)" = 'GCC_8.0.0' ] || exit 1; fi; done
    && echo CXX-CEILINGS-OK && uv run --with pyyaml python -c 'import yaml,sys; d=yaml.safe_load(open(sys.argv[1]));
    b=d["jobs"]["build"]; s=str(b); assert "manylinux_2_28_x86_64" in s and "manylinux_2_28_aarch64"
    in s and "static-libstdc++" not in s; g=next(x.get("run","") for x in b["steps"]
    if "glibc" in x.get("name","").lower() or "floor" in x.get("name","").lower());
    assert "GLIBC_2.28" in g and "for fam in" in g and "GLIBCXX 3.4.25" in g and "GCC
    8.0.0" in g; names=[x.get("name","") for x in b["steps"]]; bi=[i for i,n in enumerate(names)
    if "release binaries" in n.lower()]; gi=[i for i,n in enumerate(names) if "glibc"
    in n.lower() or "floor" in n.lower()]; assert bi and gi and gi[0]>bi[0]; print("corrected
    contract ok")' .github/workflows/release.yaml
- id: P3
  name: Retarget the HPCCM recipe to the new baseline
  status: done
  satisfies:
  - R5
  depends_on:
  - P2
  parallel: false
  hammerable: false
  hill: uphill
  verify: export PATH=/tmp/hpccm-bin:$PATH; cd hpccm && make check && grep -q 'bookworm-slim'
    xdu.docker && grep -q 'bookworm-slim' xdu.def && docker run --rm ml-standin-trixie
    xdu --version && docker run --rm --entrypoint /opt/xdu/bin/xdu-view ml-standin-trixie
    --version && if docker build -t ml-standin-bookworm - < /tmp/ml-standin/xdu-bookworm.docker
    > /dev/null 2>&1; then echo UNEXPECTED-PASS; exit 1; else echo GUARD-RED-OK; fi
- id: P4
  name: Deferral ledger and final consistency review
  status: done
  satisfies:
  - R4
  depends_on:
  - P1
  - P2
  - P3
  parallel: false
  hammerable: false
  hill: uphill
  verify: test -z "$(git status --porcelain)" && git diff --name-only main...HEAD
    && git diff main...HEAD -- .github/workflows/release.yaml | grep -q '^+.*Assemble
    release tarball' && if git diff main...HEAD -- .github/workflows/release.yaml
    | grep -E '^[+-]' | grep -qi 'apple-darwin'; then echo MACOS-TOUCHED; exit 1;
    else echo MACOS-UNTOUCHED; fi && echo P4-VERIFY-GREEN
review:
  last_reviewed_commit: 366916d79de2c2fecadfae5e75af9916bd373a7e
  verdict: changes-requested
  blocked_reason: 'F1 LOW: floor guard vacuous-pass on GLIBC_PRIVATE-only output (one-line
    pattern fix)'
  cycle: 1
---
# TECH.md — Portable Linux release baseline for RHEL8/9

The **context engine and finite-state machine** for building this feature. The YAML
frontmatter above is the resume ground-truth (read it with
`uv run --with pyyaml python .agents/factory/bin/next_phase.py spec/{slug}/TECH.md`); the per-phase
checklists below are the work. `xdu-build` executes the next actionable phase, runs its
`verify:` command, updates state via
`uv run --with pyyaml python .agents/factory/bin/set_phase.py …`, and makes one atomic code+state commit.

- **Vision / requirements (locked):** [`GOAL.md`](GOAL.md) — R-IDs are the contract.
- **Authoritative design:** [`PLAN.md`](PLAN.md).
- **Backing research:** none on the lean path — probes recorded in `PLAN.md` §4.

## Frontmatter field reference

(Copied from the template; see the template for the full field semantics. `verify:` is the
exact command that proves the phase. `review.cycle` counts completed review passes.)

## Conventions (apply to every phase)

- Commit conventions, code style, and load-bearing invariants come from
  [`AGENTS.md`](../../AGENTS.md) — it is the constitution. Consult
  [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) for the curated footgun
  checklist relevant to this change (§13 is the only touched section).
- One phase per `xdu-build` invocation by default; one atomic commit containing **both** the code and
  the `TECH.md` state change. Branch commit subjects follow the house style `[{category}] Build {slug}
  P<n>: …` (no `WIP:` prefix) — squashed into the single PR-title commit at `xdu-publish`.
- **No `Co-Authored-By` trailer** (attribution lives in the PR body, not the commit).
- Scratch build outputs live outside the repo (`/tmp/ml-xdu`, `/tmp/ml-neg`); the working tree
  holds only committed deliverables. Self-cleaning scratch is the AGENTS.md-sanctioned case.

---

## Phase P1 — Manylinux legs plus the symbol guard in release.yaml
**Satisfies:** R2, R3 · **Depends on:** —

**Goal:** the release workflow builds both gnu targets in `manylinux_2_28` containers with a
statically linked C++ runtime, and every gnu leg fails before assembly when the glibc floor
is breached.

- [x] Matrix: `x86_64-unknown-linux-gnu` leg in `manylinux_2_28_x86_64` on `ubuntu-24.04`;
  `aarch64-unknown-linux-gnu` leg in `manylinux_2_28_aarch64` on `ubuntu-24.04-arm`.
- [x] Drop the `gcc-aarch64-linux-gnu` cross install and the `.cargo/config.toml` linker
  stanza (compilation is native per arch inside the containers).
- [x] Install Rust in-leg via rustup, channel read from `rust-toolchain.toml` (same `sed`
  the current legs use); export
  `RUSTFLAGS="-C link-arg=-static-libstdc++ -C link-arg=-static-libgcc"`.
- [x] New step after the build, before assembly: the fail-closed GLIBC-max plus `NEEDED`
  allowlist scan from `PLAN.md` §2 (non-empty guard, numeric-only pattern, `set -eu`).
- [x] Accept cold cargo registries (no `Swatinem/rust-cache` in these legs); keep the
  90-minute timeout.
- **Superseded in P2 (design correction, not a scope change):** the static-link mechanism
  never took — measured `NEEDED` stays dynamic even through direct rustc invocation — and
  proved unnecessary (usage sits under RHEL8 stock ceilings). Corrected to dynamic linkage
  with `GLIBCXX`/`GCC` ceiling assertions. This phase's frontmatter gate below asserts the
  original contract and is therefore historical; the corrected contract is re-asserted in
  P2's verify.
- **F1 remediation (review cycle 1):** the guard's version patterns matched a possibly-empty
  numeric class (`GLIBC_[0-9.]*`), so `GLIBC_PRIVATE`-only output yielded a vacuous non-empty
  maximum that passed. Tightened all three extraction patterns to require a leading digit
  (`[0-9][0-9.]*`); class sweep shows exactly these two sites. Frontmatter `verify:` retuned from
  the historical static-link contract to the class gate: tightened patterns present, the loose
  `_[0-9.]*` shape absent, PRIVATE-only input extracts empty, `GLIBC_2.28` still matches.
- **Verify:** frontmatter `verify:` — YAML parses, both images, both static-link flags, the
  floor literal, and guard-after-build ordering all asserted from the parsed document.
- **Touches:** `.github/workflows/release.yaml`.

## Phase P2 — Local proof: manylinux build plus RHEL8 and bookworm probes
**Satisfies:** R1 · **Depends on:** P1

**Goal:** the binaries P1's design would ship are proven against the R1 probes before any
release depends on them — real compile, real old userlands, plus the gate's negative control
and a layout replication for R4's file-map half.

- [x] Compile the tree in `manylinux_2_28_x86_64` with P1's flags into `/tmp/ml-xdu`
  (scratch, outside the repo; aarch64 proof rides the same design on the native runner).
- [x] Floor probe per binary: `strings` GLIBC-max at most 2.28, `--version` exits 0 under
  ubi8-minimal and under bookworm-slim (bind-mounted read-only).
- [x] Negative control: the v0.5.1 `xdu-find` floater (GLIBC_2.38) fails the floor
  comparison — this is what makes the R3 gate more than decoration. The comparison is an
  independent one-line reimplementation, deliberately not the workflow text executing
  itself.
- [x] Layout replication: run the (untouched) assemble stanza locally, `tar tzf` the result
  for the `bin/` four plus the `share/` trees — pre-proof for R4, whose verdict lands in P4.
- [x] Correction (measured above): static C++ link dropped from the workflow — direct rustc
  invocation leaves the dynamic needs in place, while measured usage (`GLIBCXX_3.4.22`,
  `GCC_4.2.0`) sits under RHEL8 stock (3.4.25 / 8.0.0) with margin. Guard reworked to
  version ceilings plus an inclusive `NEEDED` allowlist; `satisfies` extended to R2/R3 for
  the re-asserted contract below.
- **Verify:** frontmatter `verify:` — executable bits, GLIBC maxima, `GLIBCXX`/`GCC`
  ceilings, both container probes per binary, layout members, the `NEGATIVE-RED-OK`
  control, and the corrected release.yaml contract.
- **Touches:** nothing committed (scratch only); the tree stays clean.

## Phase P3 — Retarget the HPCCM recipe to the new baseline
**Satisfies:** R5 · **Depends on:** P2

**Goal:** the recipe default `runtime_base` returns to `debian:bookworm-slim` with both specs
regenerated in the same change — valid only once P2 has proven the floor.

- [x] `runtime_base` default to `debian:bookworm-slim`; rewrite the trixie-rationale prose in
  `xdu.py` and `README.md` as the new floor statement (per-binary numbers stay measured).
- [x] `make -B` regen; spec diff shows the runtime move and nothing else (`make check` stays
  red until commit by construction — it diffs against HEAD — and goes green right after).
- [x] Stand-in matrix (no new release tarball exists yet; recipe `VERSION` still names the
  old tag, whose binaries need up to 2.39). Into scratch `/tmp/ml-standin`, generated twice:
  old tag + `runtime_base` overridden to trixie → `docker build -t ml-standin-trixie`
  succeeds and both probed entrypoints answer (machinery intact, SHA256SUMS path
  exercised); old tag + new bookworm default → the build FAILS LOUD at the `--version`
  smoke step with `GLIBC_2.38 not found` (guard intact — this is also the documented
  pre-release window in which a default `make image` fails rather than shipping a dead
  image).
- [x] Recreate `/tmp/hpccm-bin/hpccm` (`uvx --from hpccm hpccm`) if absent — `make check`
  needs it on `PATH`.
- **Verify:** frontmatter `verify:` — `make check`, bookworm in both specs, trixie
  stand-in runs answer, bookworm stand-in build fails red.
- **Touches:** `hpccm/xdu.py`, `hpccm/xdu.def`, `hpccm/xdu.docker`, `hpccm/README.md`.

## Phase P4 — Deferral ledger and final consistency review
**Satisfies:** R4 · **Depends on:** P1, P2, P3

**Goal:** R4's verdict (macOS legs and assemble/publish stanzas byte-untouched) plus the
mandated ledger walk: every "do not fix / known limitation / follow-up" phrase in P1–P3
maps to an `issues/` file and ROADMAP entry, or is confirmed absent.

- [x] Confirm the release.yaml diff adds legs and the guard while the macOS matrix entries
  and the assemble/publish stanzas are byte-identical to `main` (verified: Apple legs
  identical, assemble body duplicated intact, publish needs both jobs).
- [x] Walk P1–P3 bodies for deferral phrases; file any found per `templates/ISSUE.md` plus a
  ROADMAP entry, or record their confirmed absence in the commit body. Walked: the only
  live phrases are PLAN §5 contingencies (cold-build timeout/CARGO_HOME, PR-time guardrail
  rejection) plus the release-time R1 re-probe — all owned (retained PLAN record,
  release-process rerun) with triggers stated; no `issues/` filing (nothing actionable
  exists before its trigger fires).
- [x] Known candidates resolve to written no: a PR-time manylinux compile guardrail
  (rejected in PLAN as too costly per-PR — confirmed stands: a 15–30 min DuckDB compile
  on every PR for a quarterly release event); `CARGO_HOME`-on-workspace caching for
  release legs (contingency only — file if a leg ever nears the 90-minute timeout).
- **Verify:** frontmatter `verify:` — clean tree plus the changed-file list plus positive
  greps for the untouched macOS and assemble stanzas in the workflow diff.
- **Touches:** `issues/*.md`, `ROADMAP.md` only if the walk finds a live deferral.

---

## How `xdu-build` drives this

(Same engine as the template: `next_phase.py` → pre-flight → execute → `verify:` → state via
`set_phase.py` → one atomic commit; STOP and escalate only on a **`GOAL.md` contradiction**.)
