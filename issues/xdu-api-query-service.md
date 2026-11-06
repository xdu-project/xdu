---
status: unshaped
kind: feature
appetite: big
---

# `xdu-api`: the shared-index query service

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

An index built as root across a whole center describes every tenant on it, and nothing in the
current tool chain can hand a tenant only their slice. DuckDB has no user model, no accounts and no
row-level security; it is a library running inside the caller's process with the caller's
credentials, so a filter applied client-side is a convenience and not a boundary. Anyone who can
open the index files reads all of them, whatever the reader was asked to display.

That leaves exactly two enforcement points: the operating system, or a process the caller does not
control. The OS route was explored and rejected in the same design pass — file modes express unions
and not the intersections real permission chains produce, and a mode baked at crawl time keeps
granting access after the underlying directory is locked down, durably, because a reader can copy
the chunk while it is still readable.

## Why it was deferred

New work, not a factory-pass deferral. It is the core of the 2026-09-11 redesign of
[`access-scoped-queries.md`](access-scoped-queries.md). Architecturally the largest item in that
stack: expect dedicated research and sub-phases at promotion.

## Outcome / vision

`xdu-api` is the only reader with access to a shared index. It authenticates the caller, resolves
them to a set of visibility classes, answers typed API operations with rows scoped to that set, and
records what it answered. The index is read-only, so the service is stateless: N replicas behind a
load balancer, no coordination.

DuckDB runs hardened and locked — `enable_external_access=false`, `allowed_directories`, extension
autoload/autoinstall/community off, then `lock_configuration=true`, with the index attached *before*
the lock. Disabling external access and reading an index over the network are mutually exclusive,
so this sketch assumes local copies and treats object storage as the distribution mechanism. That
is a trade to decide here on evidence, not a constraint inherited from elsewhere: the service
builds its own queries rather than accepting SQL, so how much the hardened posture is buying
against how much a per-replica copy costs is an open question for promotion. There is no
statement-timeout setting, so query runtime is bounded by interrupting from the handler.

Aggregates report what the caller can see, matching `du` semantics, and count what was withheld.
Reporting a true total across invisible rows would turn the service into a differencing oracle —
watch a subtree total move across generations and learn about files you cannot see — and reporting a
filtered total silently would contradict `lfs quota` with no explanation. Stating both numbers is
the honest option.

Serving queries rather than handing out data also means grants are revocable: nothing is disclosed
permanently, so permission staleness collapses to the crawl interval plus the group-cache TTL.

## Sketch of the acceptance criteria

Draft R-IDs, to be firmed up at promotion.

- **R1** — The index store *shall not* be readable by the principals the service serves; `xdu-api`
  *shall* be the only reader with access to it.
- **R2** — *When* an authenticated caller issues an API operation, `xdu-api` *shall* return only
  rows whose visibility class is in the class set resolved for that caller.
- **R3** — *While* serving, the DuckDB connection *shall* have external access disabled and its
  configuration locked, so no operation can read or write a path outside the index.
- **R4** — *If* an operation exceeds its time or memory budget, *then* `xdu-api` *shall* interrupt
  it and return an error rather than let it run.
- **R5** — Aggregates *shall* be computed over the caller's visible rows only and *shall* report the
  count of entries withheld.
- **R6** — `xdu-api` *shall* record an audit entry per query: principal, operation, index
  generation, timestamp.
- **R7** — A bulk result *shall* be paginated under a documented cap rather than streamed unbounded.

## Notes

- Depends on: [`index-visibility-classification.md`](index-visibility-classification.md),
  [`shared-index-api-contract.md`](shared-index-api-contract.md),
  [`shared-index-authentication.md`](shared-index-authentication.md), and S3 as an index target for
  distribution (met, on `main` since #33).
- Related: [`shared-index-deployment-guards.md`](shared-index-deployment-guards.md) enforces R1 at
  startup.
- Found by: shared-index design pass, 2026-09-11.
