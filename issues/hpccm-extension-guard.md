---
status: unshaped
kind: fix
appetite: small
---

# HPCCM recipe: assert the image carries the shipped DuckDB extensions

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

Release tarballs now carry the pinned DuckDB extensions at `lib/xdu/extensions/`, and the tools load
them from `<prefix>/lib/xdu/extensions/` beside `<prefix>/bin/`, with no download fallback. The HPCCM
recipe (`hpccm/xdu.py`) extracts the whole tarball into the prefix, so `lib/` lands in the right
place by construction. But nothing in the recipe asserts it. Its build-time smoke runs each binary's
`--version`, which proves the glibc floor and nothing about the extensions. A tarball that lost its
`lib/`, or a prefix change that split `bin/` from `lib/`, would build an image whose first S3 query
fails, and the build would stay green.

## Why it was deferred

The guard cannot land before a release ships `lib/`. The recipe pins `VERSION = '0.5.2'`, a tarball
with no `lib/`, so a `test -f` on the extension would break the recipe on `main` until the next
release. The guard is therefore due **in the release commit that first moves `VERSION` to a tarball
carrying `lib/`**: `/xdu-release` already bumps `VERSION` in lockstep with `Cargo.toml` and carries
the regenerated `xdu.def` and `xdu.docker` in the same commit. This is not pre-existing; the
extension-distribution cycle introduced the layout the guard checks. Its adversarial review
recorded the gap as a low finding and deferred it here.

## Outcome / vision

An HPCCM-built image (Docker or Apptainer) cannot build unless the extensions sit where the binaries
look for them, the same way the repository's own `Dockerfile` and every release leg already fail.

## Sketch of the acceptance criteria

- **R1** — WHEN the recipe builds an image from a release whose tarball carries `lib/xdu/extensions/`,
  the build SHALL fail unless `{prefix}/lib/xdu/extensions/httpfs.duckdb_extension` exists after
  extraction. The natural home is the existing `--version` smoke shell, as one more command:
  `test -f {prefix}/lib/xdu/extensions/httpfs.duckdb_extension`.
- **R2** — The guard SHALL land in the same commit that moves the recipe's `VERSION` to the first
  such release, with `xdu.def` and `xdu.docker` regenerated (`make check` clean), and the release
  skill SHALL name the step, so the bump cannot ship without it.
- **R3** — WHEN an operator overrides `version=` to a release older than the first one with `lib/`,
  the recipe SHALL either still build (the guard keyed to the release) or refuse with a message
  naming the reason. Which of the two is a shaping decision.

## Notes

- Related: `spec/duckdb-extension-distribution/` (the layout, on `main` since #31);
  `hpccm/xdu.py`; the `Dockerfile`'s runtime-stage `test -f`, which is the same guard for the
  repository's own image.
- The pinned set is a list (`xdu::engine::PINNED_EXTENSIONS`). If a later cycle adds an extension,
  a guard on `httpfs` alone still passes; consider checking every pinned name, or the directory
  being non-empty.
- Found by: `duckdb-extension-distribution` planning (adversarial review, finding 13), recorded in P4.
