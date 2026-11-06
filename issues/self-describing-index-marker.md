---
status: unshaped
kind: refactor
appetite: small
---

# A self-describing index marker

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

The marker says what version an index is and nothing about how it is shaped. `lib::index_glob`
hard-codes `<index>/*/*.parquet`, so the layout is knowledge held in the readers rather than a fact
recorded by the writer. Any index with a different shape fails on a glob that matched no files —
loud, but naming the symptom instead of the cause.

The version gate compounds it. `lib::index_version_error` refuses anything that is not exactly
`INDEX_FORMAT_VERSION`, which is right for a reader meeting a *newer* index and wrong for a reader
meeting an older one. As written, every future layout choice is a hard break in both directions: a
reader taught a new layout can no longer read the indices already on disk, so shipping one is a
flag day across every host that holds an index.

Neither is a defect today, because there is exactly one layout. Both become defects the moment there
are two, which is what [`opt-in-deeper-partitioning.md`](opt-in-deeper-partitioning.md) and
[`xdu-repack.md`](xdu-repack.md) propose.

## Why it was deferred

Not deferred from a factory pass. Separated 2026-09-15 from the layout work that needs it, because
it is a contract rather than a feature: it decides what a marker key *means* and what a reader owes
one it does not recognise. Folding that into whichever layout cycle happens to land first would
settle a cross-cutting question as a side effect of an unrelated decision, and the answer binds
every reader and every writer after it.

## Outcome / vision

An index describes itself. A reader loads the marker, learns the layout, builds the right glob, and
either understands what it is looking at or refuses saying so. Adding a layout stops being a flag
day: new readers keep reading old indices, old readers refuse new ones by construction, and the
refusal names the layout rather than a glob.

The marker also gains a vocabulary with two classes in it, which is the part worth getting right
once. Some keys a reader **must** understand — get them wrong and it reads the wrong rows. Others
are descriptive, and a reader that has never heard of them should carry on. Without that
distinction, the first cosmetic key added will hard-fail some reader in the field.

## Sketch of the acceptance criteria

- **R1** — The marker SHALL distinguish keys a reader must understand from keys it may ignore, such
  that an unrecognised descriptive key never causes a refusal and an unrecognised load-bearing key
  always does.
- **R2** — A reader SHALL accept any index format version it understands, not only the version it
  would itself write.
- **R3** — The marker SHALL describe the layout in enough detail for a reader to construct the
  correct read without inferring it from the directory tree.
- **R4** — IF a reader meets a layout or a load-bearing key it does not understand, THEN it SHALL
  refuse with that as the stated cause — no rows, no deletions — rather than failing on a glob that
  matched nothing.
- **R5** — The marker SHALL record provenance when an index is derived from another index rather
  than from a crawl, and a derived index SHALL carry forward the crawl-time counts (`errors`,
  `vanished`) it inherits rather than resetting them.

## Notes

- **R5 is a safety requirement, not bookkeeping.** `errors` and `vanished` describe the crawl. An
  index rewritten from one built under `--allow-errors` still knows nothing about the files that
  crawl skipped, and `xdu-rm`'s risk is precisely the files an index does not know about. A rewrite
  that resets those counts to zero deletes the warning
  (`lib::index_completion_warning`) that the operator running `xdu-rm` weeks later depends on.
- **R2 is what makes the whole layout theme shippable.** Without it, an opt-in layout is not opt-in:
  upgrading a reader to understand it breaks every existing index on the host.
- **Keep the scheme structured.** `partition_depth=2` plus a named key reads back as data; one
  opaque layout string reads back as something to parse and get wrong, and R4's clean refusal
  depends on being able to tell "understood and different" from "not understood".
- **Ordering.** Prerequisite for [`opt-in-deeper-partitioning.md`](opt-in-deeper-partitioning.md)
  and for [`xdu-repack.md`](xdu-repack.md) writing markers that describe what it produced.
  [`chunk-clustering-and-row-groups.md`](chunk-clustering-and-row-groups.md) deliberately does
  **not** depend on it: its marker key is descriptive, so it can land first and be re-classified
  here.
- **Adjacent, not included.** The marker's counts come from one run even when a
  `--partition`-scoped run rewrites it, so it already overstates what it attests
  ([`marker-scoped-run-attestation.md`](marker-scoped-run-attestation.md)). That is a separate
  defect in the same file; this seed changes the marker's vocabulary, not the accuracy of its
  counts.
- **The S3 marker cap is settled.** The S3 index target (on `main` since #33, record in
  `spec/s3-index-target/`) caps the marker read over S3 too: a HEAD reads the object's size, and a
  marker over `MARKER_READ_LIMIT` is refused as `MarkerRead::Oversized` without its GET. Growing the
  marker's contents has to stay inside that cap, which is now a limit on this seed in both
  directions rather than an open question.
- Found by: the DuckDB-native S3 read spike, 2026-09-15.
