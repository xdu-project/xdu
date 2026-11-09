---
status: unshaped
kind: fix
appetite: small
---

# The README's raw DuckDB queries read a `key=value` directory name as a column

> **Pre-shaped candidate, not a contract.** This file records deferred work in enough detail that a
> future session does not have to re-derive it. It is **not** graded by `xdu-review` and must never be
> copied into `spec/{slug}/GOAL.md` verbatim — `/xdu-feature` promotes it, and that is where appetite,
> non-goals and the R-IDs get negotiated with a human. The `status:` field above is the guard, with
> three states: `unshaped` means proposal; `shaped on <branch> (<date>) — see spec/<slug>/GOAL.md`
> means a cycle adopted it and is still in flight; `resolved on <branch> (<date>) — see
> spec/<slug>/` means the cycle landed on `main`. Only `/xdu-feature` writes `shaped`, only the
> landing cycle writes `resolved`, and `/xdu-roadmap` deletes the file once the landing is confirmed.
>
> Deliberately **not** named `GOAL-{slug}.md`: every other GOAL in the factory is a locked contract, so
> a file carrying that name eventually gets treated as one.

**Priority: low.** No xdu tool is affected; a user who queries an index by hand is.

## Problem

Every xdu reader now reads an index with `hive_partitioning = false`, so a `key=value` directory
name is read as a name: `xdu-find`, `xdu-rm` and `xdu-view` since #38, the select-act engine behind
`xdu-cp` and `xdu-mv` since #37 (`AGENTS.md` § *Project*). The two places a user learns to read an
index without xdu still describe or use DuckDB's default, which turns the detection on:

- `README.md` § *Querying with DuckDB* (`:469`) gives three raw queries, over
  `read_parquet('/var/lib/xdu/home/*/*.parquet')` (`:478`, `:484`) and
  `read_parquet('/var/lib/xdu/home/alice/*.parquet')` (`:491`). When every path a glob matches
  carries a `key=value` directory, DuckDB adds a column of that name holding the directory's value,
  and it shadows the column the crawler wrote. An index stored under a directory such as
  `…/mtime=0/` does that to all three queries; a partition named `size=0` does it to the
  single-partition one. The "files not accessed in 180 days" query then filters on the directory
  name, not on the row. The mechanism is the one reproduced for the tools in
  `spec/hive-partition-columns-find-rm/`; it has not been re-run against the README text.
- `www/index.html` calls the index "Hive-partitioned" in the meta description (`:8`), the overview
  (`:390`) and the layout paragraph (`:619`), heads that section "Hive layout" (`:597`), and says
  "the Hive layout maps onto object-store key prefixes" in the S3 roadmap item (`:816`). The layout
  is one directory per top-level subdirectory under its bare name (`bioinfo/`, not
  `user=bioinfo/`), and no xdu reader applies Hive semantics to it. A reader who takes the word at
  face value queries with detection on, which is the shadowing above, or looks for a partition
  column no xdu index carries.

## Why it was deferred

Outside the GOAL of `hive-partition-columns-find-rm`, which fixed the readers xdu ships. Its plan
named both places for the foreman and deferred nothing to them
(`spec/hive-partition-columns-find-rm/PLAN.md` §5). **Pre-existing**: the README examples and the
site wording predate that cycle, which changed neither.

## Outcome / vision

A user who copies a README query gets the rows `xdu-find` would select, whatever the directories
are named. The site describes the layout as what it is and promises nothing xdu does not apply.

## Sketch of the acceptance criteria

Draft R-IDs, to be firmed up at promotion. Prefer EARS phrasing (see
[`.agents/factory/ears.md`](../.agents/factory/ears.md)).

- **R1** — WHEN a user runs a `README.md` DuckDB example against an index whose path or partition
  carries a `key=value` directory name, the query SHALL read that name as a name and filter on the
  columns the crawler wrote (for instance by passing `hive_partitioning = false` to each
  `read_parquet`), or the README SHALL say beside the examples why it does not and what such a name
  does to them.
- **R2** — `www/index.html` SHALL NOT describe the index in terms that promise Hive semantics xdu no
  longer applies: no wording a reader could take to mean that a `key=value` directory becomes a
  column.

## Notes

- Shaping decides whether the site keeps "Hive" for the directory shape, qualified, or drops it.
  `www/` deploys on every push to `main` (`.github/workflows/deploy-pages.yaml`), and this change
  describes behavior already on `main`, so unlike `www-cp-mv-at-release.md` it waits on no release.
- Related: `spec/hive-partition-columns-find-rm/` (PLAN §5; GOAL non-goals, where `key=value`
  source names are ruled legitimate); [`www-cp-mv-at-release.md`](www-cp-mv-at-release.md), the
  other pending edit to `www/index.html`; [`toward-v1-0-release.md`](toward-v1-0-release.md), which
  reworks the README.
- Found by: `hive-partition-columns-find-rm` plan (PLAN §5), filed at the retirement of its seed,
  2026-11-09.
