---
status: shaped on feature/duckdb-extension-distribution (2026-11-04) — see spec/duckdb-extension-distribution/GOAL.md
kind: feature
appetite: small
---

# Ship DuckDB extensions instead of downloading them

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

Every DuckDB capability xdu does not already link statically arrives as a runtime download into
`$HOME/.duckdb/extensions/`, and that is a product defect on the hosts xdu targets. `Cargo.toml`
asks `duckdb` for `parquet` precisely to stop the Parquet reader autoinstalling a 12 MB extension on
an air-gapped login node, and `tests/offline_tests.rs` holds that line by redirecting `HOME` at an
empty directory and asserting it stays empty.

The linkage escape hatch does not generalize. Measured 2026-09-15: `libduckdb-sys` 1.4.4 vendors an
amalgamation whose `duckdb/manifest.json` carries sources for exactly `core_functions`, `parquet`
and `json` — so no third extension can be statically linked, whatever Cargo features are set.
Anything else has to arrive as a binary.

Two queued seeds are blocked on the same wall for the same reason.
[`s3-index-target.md`](s3-index-target.md) needs `httpfs` to read an index over ranged S3 requests,
and [`duckdb-fts-evaluation.md`](duckdb-fts-evaluation.md) already records the constraint in its own
words: anything arriving as a runtime extension download "restores a product defect on HPC login
nodes rather than a feature." Solving it once unblocks both, and every extension after them.

## Why it was deferred

Not deferred from a factory pass. Extracted 2026-09-15 from the S3 spike that measured the
mechanics, because it is prerequisite infrastructure with a consumer other than S3 and a blast
radius — the release tarball layout, `install.sh`, the `Dockerfile`, and the offline guarantee —
entirely unlike the feature that first needed it. The tarball layout is called a contract in
`AGENTS.md`, so changing it deserves its own cycle rather than a rider on a transport change.

## Outcome / vision

An operator installs xdu and the extensions it needs are already on disk, pinned to the engine
version the binary was built against. Nothing downloads, nothing writes to `$HOME`, and an
air-gapped host behaves exactly like a connected one. When an extension is genuinely absent, the
tool says which file it looked for instead of reaching for the network.

The spike settled the mechanics, so the uncertainty here is packaging rather than feasibility.
Measured on darwin/arm64 against the bundled engine: `SET extension_directory='<dir>'` redirects
installs; a pre-seeded directory plus `LOAD httpfs` succeeds with no network and leaves a cold
`HOME` empty; `LOAD '<absolute path>.duckdb_extension'` loads a vendored binary directly; and with
`autoinstall_known_extensions=false` a missing extension fails as `IO Error: Extension "<exact
path>" not found.` The official `linux_amd64` `httpfs` requires no glibc symbol above `GLIBC_2.28`,
which is the manylinux_2_28 floor the v0.5.2 release baseline already targets.

## Sketch of the acceptance criteria

- **R1** — The release tarball SHALL carry the DuckDB extensions xdu requires, built for the same
  engine version and platform as the binaries beside them, and `install.sh` SHALL place them where
  the tools find them without further configuration.
- **R2** — WHEN a tool needs an extension, it SHALL load it from a pinned local location with no
  network access and no write to `$HOME`, honoring an operator override for hosts that install
  elsewhere.
- **R3** — IF a required extension is absent, THEN the tool SHALL fail with a message naming the
  path it looked for, rather than attempting a download.
- **R4** — The local-index query path SHALL load no extension it does not already link statically,
  and the offline guarantee SHALL be asserted for the shipped build, not only for the current
  dependency line.

## Notes

- Autoinstall and autoload both default to on in the bundled build (`libduckdb-sys` `build.rs`
  hard-codes `DUCKDB_EXTENSION_AUTOINSTALL_DEFAULT=1` and `DUCKDB_EXTENSION_AUTOLOAD_DEFAULT=1`), so
  the offline posture today is the absence of a trigger rather than a refusal. Turning both off
  explicitly makes R4 an assertion instead of an accident.
- Extension binaries are version-locked to the engine. A `duckdb` crate bump moves the pinned set,
  which makes this a release-workflow concern and not only an install-time one.
- Size is the cost to weigh: `httpfs` for `linux_amd64` is 24 MiB raw, 8.8 MiB gzipped, per platform
  leg. Whether every leg ships every extension, or the set is per-target, is a promotion question.
- `PROVIDER credential_chain` needs a second extension (`aws`) on top of `httpfs`; resolving the
  chain in Rust and passing explicit credentials avoids shipping it. That trade belongs to
  [`s3-index-target.md`](s3-index-target.md), but it decides how many binaries this seed ships.
- Related: [`s3-index-target.md`](s3-index-target.md) (first consumer),
  [`duckdb-fts-evaluation.md`](duckdb-fts-evaluation.md) (second, blocked on the identical
  constraint), [`native-os-packages-deb-rpm.md`](native-os-packages-deb-rpm.md) (inherits whatever
  layout this establishes).
- Found by: the DuckDB-native S3 read spike, 2026-09-15.
