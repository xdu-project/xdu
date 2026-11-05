---
status: unshaped
kind: feature
appetite: small
---

# DuckDB full-text search evaluation

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

There is no content-aware search: the index holds pathnames, sizes, atimes, ownership, permissions,
and change times, and every query is a predicate over those columns. Whether DuckDB's full-text search machinery
fits this product — as a richer query option over paths, now, or over file contents later —
has never been evaluated, only named as a direction.

## Why it was deferred

GOAL non-goal of the glob pilot (`spec/richer-search-glob-fuzzy-fulltext/GOAL.md`), with an
agreed first step: a written evaluation (spike report), not a shipped query path. The
evaluation has a known hard constraint to work through — `tests/offline_tests.rs` guards
air-gapped operation, so anything arriving as a runtime extension download (the failure
mode `Cargo.toml`'s `duckdb/parquet` feature exists to prevent) restores a product defect
on HPC login nodes rather than a feature.

## Outcome / vision

A committed evaluation that concludes whether DuckDB full-text search fits: what it would
index, how it ships (bundled vs extension, and the offline story), and what it costs at
scale. A "no" with reasons is a complete outcome.

## Sketch of the acceptance criteria

- **R1** — The evaluation SHALL be committed as a document weighing fit, packaging, and
  scale cost, ending in a build-or-drop recommendation.
- **R2** — IF the recommendation is to build, THEN the document SHALL state the phased
  shape of that work rather than starting it.

## Notes

- The extension-download constraint above is no longer this seed's to solve: the shipped-extensions
  cycle (on `main` since #31, record in `spec/duckdb-extension-distribution/`) pins DuckDB
  extensions to the engine version and ships them in the release tarball, having measured that no
  extension beyond `core_functions`/`parquet`/`json` can be statically linked at all. If full-text
  search ships as an extension, it ships through that mechanism as one more pinned-set entry.
- Related: the retired parent seed `issues/richer-search-glob-fuzzy-fulltext.md` (glob pilot
  delivered; record in `spec/richer-search-glob-fuzzy-fulltext/`).
- Found by: `richer-search-glob-fuzzy-fulltext` P4 deferral ledger.
