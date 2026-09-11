---
status: shaped on feature/manylinux-release-baseline (2026-09-11) — see spec/manylinux-release-baseline/GOAL.md
kind: feature
appetite: small
---

# Release binaries target a manylinux-class glibc baseline for RHEL8/9

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

The Linux release tarballs are built on `ubuntu-24.04`
(`.github/workflows/release.yaml:94-98`) and need glibc 2.38 (`xdu-find`,
`xdu-rm`) to 2.39 (`xdu-view`), per `hpccm/README.md:51-53` and
`hpccm/xdu.py:19-21`. They fail at first exec on anything older with
`GLIBC_2.38 not found` or equivalent: RHEL8 ships 2.28, RHEL9 ships 2.34,
Debian bookworm ships 2.36. The HPC sites this project serves run RHEL8/9,
so the native tarball is unusable there and the container is the only path.
The HPCCM recipe records the workaround directly: it takes a
`debian:trixie-slim` (2.41) runtime because bookworm cannot run the binaries,
while the top-level `Dockerfile` is unaffected because it compiles against
bookworm's own glibc.

Moving the builder to `ubuntu-22.04` does not fix this. Jammy ships glibc
2.35, still newer than RHEL9 (2.34) and RHEL8 (2.28). A baseline at or below
2.28 covers both, which in practice means a `manylinux_2_28`-class builder
image rather than a newer GitHub-hosted runner.

## Why it was deferred

Found in review of the HPCCM container PR (`pr/lgorenstein/23`), which works
around the gap with a trixie base instead of fixing it. The gap is
**pre-existing** on `main`: the release matrix already built on 24.04 before
that branch. Fixing it means moving the gnu build legs into an older-glibc
container, re-validating the bundled-DuckDB C++ compile and the aarch64
cross toolchain there, and adding a guard so the baseline cannot float back
with the next toolchain bump. That is release-topology work with its own
verification, not a rider on a container-definition change.

## Outcome / vision

The Linux gnu tarballs run on RHEL8 and newer without a container. The HPCCM
recipe tracks the new baseline (a bookworm-class runtime again) and remains
for what it is for — Apptainer packaging with no toolchain — rather than as
a glibc workaround. macOS targets and the `bin/` + `share/` tarball layout
are unchanged.

## Sketch of the acceptance criteria

Draft R-IDs, to be firmed up at promotion.

- **R1** — WHEN a user extracts a Linux gnu release tarball on RHEL8, RHEL9,
  or Debian bookworm, THEN all four binaries SHALL print `--version` and exit
  0 with no `GLIBC_* not found` error.
- **R2** — The release workflow SHALL build the gnu targets in a builder whose
  glibc is at most 2.28, so the floor covers RHEL8 (2.28) and everything newer.
- **R3** — CI SHALL assert the gnu binaries require no glibc symbol newer than
  the baseline (for example by scanning dynamic symbol versions), so a Rust or
  DuckDB toolchain bump that floats the floor fails the build instead of
  shipping a tarball that dies on first exec.
- **R4** — The macOS targets and the tarball file map SHALL be unchanged by the
  builder move.
- **R5** — The HPCCM recipe default `runtime_base` SHALL track the new baseline
  rather than trixie, or record why trixie is retained.

## Notes

- Related: `pr/lgorenstein/23` (the HPCCM recipe that documents the workaround
  at `hpccm/xdu.py:19-21` and `hpccm/README.md:51-57`); the top-level
  `Dockerfile:21,56` (bookworm builder/runtime, unaffected);
  `rust-toolchain.toml:13` (pinned Rust 1.97.1, must run under the older
  builder); `issues/native-os-packages-deb-rpm.md` (adjacent packaging work).
- Shaping decision, not established here: a fully static musl target would also
  remove the glibc floor, but trades the dynamic `libstdc++`/DuckDB posture the
  `Dockerfile` currently pins. Promotion evaluates musl against manylinux and
  picks one.
- Found by: review of `pr/lgorenstein/23`, 2026-09-10.
