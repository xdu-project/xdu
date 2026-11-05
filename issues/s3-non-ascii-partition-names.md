---
status: unshaped
kind: feature
appetite: small
---

# Index top-level directories with non-ASCII names into S3

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu -o s3://bucket/prefix` refuses a run whose source has a top-level directory with a non-ASCII
name, such as `café`. It names the directory and exits non-zero before clearing the marker or
writing anything. A local run indexes the same tree without complaint.

The refusal follows from how keys are built. Each top-level directory is a partition, and its name
becomes a key segment. The writer's client, `object_store`, percent-encodes every non-ASCII byte in
a key path, so the stored key would differ from the name the readers put in their glob. The writer
therefore refuses any name the client would encode, along with `.`/`..`, control characters, glob
metacharacters and the characters AWS advises against (`S3Sink` in `src/s3.rs`).

The alternative is to build keys raw, so that a non-ASCII name is stored as its UTF-8 bytes. That
admits these names, but the reader side has never been probed: how `httpfs` lists, globs and fetches
a key with non-ASCII bytes, and whether the `filename` it reports round-trips to the partition name
`xdu-find --top` and `xdu-view` print.

## Why it was deferred

The foreman ruled at `s3-index-target` P3 run B to keep the refusal. Admitting the names needs a
reader-parity wire test first, which was out of that phase's scope. Refusing is safe because it
fails loud before anything is written. New with S3 support.

## Outcome / vision

A source tree with non-ASCII top-level names builds into S3, and every reader answers from it
exactly as from a local build of the same tree.

## Sketch of the acceptance criteria

- **R1** — WHEN a top-level directory's name is valid UTF-8 but not ASCII, `xdu -o s3://…` SHALL
  index it, storing the key so that the readers' glob and `filename` round-trip to the same name.
- **R2** — A wire test SHALL show `xdu-find` (listing, `-u`, `--top`) and `xdu-view` (partition
  list, directory listing) answer the same over S3 as locally for such a partition.
- **R3** — Names that are not valid UTF-8, or that would not round-trip, SHALL still be refused
  before the marker is cleared.

## Notes

- Related: `spec/s3-index-target/` (GOAL R1 and its clarification; TECH P3's run A amendment and
  the foreman's ruling at P3 run B).
- Found by: `s3-index-target` P3.
