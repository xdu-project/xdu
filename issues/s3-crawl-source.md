---
status: unshaped
kind: feature
appetite: big
---

# S3 as a crawl source

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

Huge datasets increasingly live in object storage — data lakes, cold archives, tiered backups —
and get none of the size/age/pattern auditing `xdu` gives a POSIX tree.

## Why it was deferred

Not deferred from a factory pass — a forward-looking intention from the original roadmap, recorded
before the `issues/` back-reference convention was enforced. This file retrofits the
back-reference; no new scoping was done. Architecturally this is the larger move: expect dedicated
research and sub-phases at promotion.

## Outcome / vision

An S3 bucket audited the same way as a local tree: the crawler abstracts behind a trait so a local
jwalk backend and an S3-listing backend are interchangeable behind one CLI, with per-source
capability differences expressed rather than assumed — object storage has no atime, so the
Unix-only assumption stops holding for every backend.

## Sketch of the acceptance criteria

- **R1** — `xdu` SHALL audit files in an S3-compatible bucket with the same size/age/pattern
  reporting as a local tree.
- **R2** — The crawler SHALL abstract local and S3 sources behind one CLI, expressing
  per-source capability differences (notably: no atime on objects).

## Notes

- Sequenced after the S3 index target, now met: on `main` since #33 (record in
  `spec/s3-index-target/`), which established the locator parsing, credential resolution and
  endpoint handling this seed reuses. The source direction still
  needs its own client for listing objects; what it inherits is the configuration surface, not the
  transport.
- Found by: original roadmap; back-reference retrofitted 2026-09-07.
