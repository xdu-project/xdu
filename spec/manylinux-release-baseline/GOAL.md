# GOAL — Portable Linux release baseline for RHEL8/9

> **Origin spec.** The *what* and *why* — the locked contract `xdu-review` grades against.
> The *how* lives in [`PLAN.md`](PLAN.md) and [`TECH.md`](TECH.md) (written by `xdu-plan`).
> Keep this at the right altitude: solved and bounded, but not over-specified — leave design
> freedom for the plan. Edit requirements here; do **not** silently drift them during build.

- **slug:** manylinux-release-baseline
- **kind:** feature
- **appetite:** small  ·  *CI topology plus a recipe retarget; no `src/` change is expected.*

## Problem

The Linux release tarballs are built on `ubuntu-24.04`
(`.github/workflows/release.yaml:94-98`) and carry that builder's glibc floor with them:
measured against v0.5.1, `xdu` needs 2.34 while `xdu-find` and `xdu-rm` need 2.38 and
`xdu-view` needs 2.39 (`hpccm/xdu.py:21-23`, `hpccm/README.md:54-55`). They fail at first
exec on anything older with `GLIBC_2.38 not found` or equivalent — RHEL8 ships 2.28, RHEL9
ships 2.34, Debian bookworm ships 2.36 — and the HPC sites this project serves run RHEL8/9.
So the native tarball is unusable exactly where the users are, and the container is the only
path. The HPCCM recipe records the workaround directly: it takes a `debian:trixie-slim`
(2.41) runtime because bookworm cannot run the binaries, while the top-level `Dockerfile`
is unaffected because it compiles against bookworm's own glibc (`Dockerfile:21,56`).

Stepping the builder down one notch does not fix this. Ubuntu 22.04 ships glibc 2.35,
still newer than RHEL9 (2.34) and RHEL8 (2.28). Only a baseline at or below 2.28 covers
both, which in practice means a `manylinux_2_28`-class builder image rather than a newer
GitHub-hosted runner. The gap is pre-existing on `main`: the release matrix already built
on 24.04 before the container work, which chose to work around it rather than fix it.

## Outcome / vision

The Linux gnu tarballs run on RHEL8 and newer with no container involved. The HPCCM recipe
tracks the new baseline (a bookworm-class runtime again) and remains for what it is for —
Apptainer packaging with no toolchain — rather than as a glibc workaround. macOS targets
and the `bin/` + `share/` tarball layout are unchanged, and the floor cannot silently float
back with the next toolchain bump.

## Acceptance criteria (the contract)

- **R1** — WHEN a user extracts a Linux gnu release tarball on RHEL8, RHEL9, or Debian
  bookworm, THEN all four binaries SHALL print `--version` and exit 0 with no `GLIBC_* not
  found` error.
- **R2** — The release workflow SHALL build the gnu targets in a builder whose glibc is at
  most 2.28, so the floor covers RHEL8 (2.28) and everything newer.
- **R3** — IF a toolchain change (Rust, DuckDB C++) raises a gnu binary's required glibc
  above the floor, THEN CI SHALL fail instead of shipping a tarball that dies on first exec.
- **R4** — The macOS targets and the tarball file map SHALL be unchanged by the builder move.
- **R5** — WHEN this ships, the HPCCM recipe default `runtime_base` SHALL track the new
  baseline rather than trixie, with both specs regenerated from the recipe in the same change.

## Non-goals (no-gos)

- A fully static musl target. It would also remove the glibc floor, but trades away the
  dynamic `libstdc++`/DuckDB posture the `Dockerfile` pins; manylinux is the decided path and
  musl stays out (decided 2026-09-11, see Clarifications).
- Changing the tarball layout, the install contract, or anything under `src/`.
- Native OS packages (`.deb`/`.rpm`) — the adjacent `issues/native-os-packages-deb-rpm.md`,
  a separate cycle.
- Non-Linux portability. The index stays Unix-only (`MetadataExt` is load-bearing); this cycle
  lowers the Linux floor, nothing more.
- Rebuilding the top-level `Dockerfile` bases. It compiles against bookworm's own glibc and is
  unaffected by this work.

## Clarifications

- **Q:** Which glibc floor should the contract demand — 2.28 (RHEL8+) or 2.34 (RHEL9+ only)?
  — **A:** 2.28. Only a floor at or below RHEL8's 2.28 covers both EL generations; a 22.04- or
  bookworm-class builder would still strand RHEL8 (resolved 2026-09-11).
- **Q:** Should a static musl target stay on the table as an alternative `/xdu-plan` evaluates?
  — **A:** No. manylinux is decided now; musl is a Non-goal above, not a plan-time option
  (resolved 2026-09-11).
- **Q:** Is retargeting the HPCCM recipe `runtime_base` acceptance criteria of this cycle, or
  deferred follow-up? — **A:** Same cycle (R5), including the spec regen (resolved 2026-09-11).

## Related materials

- Seed: `issues/manylinux-release-baseline.md` (pre-shaped Problem, deferred-reason record,
  draft R-IDs as input).
- ROADMAP entry: `ROADMAP.md` ("Portable Linux baseline (manylinux) for RHEL8/9", first entry).
- Sources: `.github/workflows/release.yaml` (gnu build legs), `hpccm/xdu.py` + `hpccm/xdu.def` +
  `hpccm/xdu.docker` (workaround + retarget), `Dockerfile` (unaffected source build),
  `rust-toolchain.toml:13` (pinned Rust 1.97.1, must run under the older builder).
- Release procedure already tracks the recipe bump: `.agents/skills/xdu-release/SKILL.md` (Step 4),
  `.agents/factory/invariants.md` §13.
