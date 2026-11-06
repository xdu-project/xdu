---
status: unshaped
kind: feature
appetite: big
---

# Web client (`xdu-web`)

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

`xdu-view` is terminal-only, which limits who can explore an index and from where: every consumer
needs a shell account on a machine holding the index.

## Why it was deferred

Not deferred from a factory pass — a forward-looking intention from the original roadmap, recorded
before the `issues/` back-reference convention was enforced. This file retrofits the back-reference;
no new scoping was done.

Revised 2026-09-11. This seed previously called for the Wasm app to browse an S3-backed index
directly from the browser. That is incompatible with a shared index: reaching the store from the
browser means either a world-readable index or credentials handed into a page the user can inspect,
and a browser has no POSIX identity with which to scope anything. The vision below routes the shared
case through the API instead — and keeps the direct-read case where it is still safe, namely an
index file the user already owns.

## Outcome / vision

A Wasm-compiled progressive web app that is the web equivalent of `xdu-view` — the same list and
tree views, the same search and filtering — in two modes that mirror the CLI's:

- **Shared index.** The app authenticates the user and issues typed API operations against
  `xdu-api`, which scopes every answer to what that user could see on the live filesystem. This is
  what makes a centrally built index explorable by anyone with a link and no shell account.
- **Local index.** The user opens an index file they already hold, and the app queries it in-browser
  with no service, no authentication and no configuration — the browser counterpart of running
  `xdu-view` on your own index, and safe for the same reason: the crawl already applied the
  permissions.

The browser is also the forcing function for the served architecture rather than a beneficiary of
it: having no POSIX identity, it cannot participate in any scheme that hands data to the client and
expects the client to filter.

## Sketch of the acceptance criteria

Draft R-IDs, to be firmed up at promotion.

- **R1** — The web app *shall* browse a shared index with list and tree views plus search, mirroring
  `xdu-view`, with no shell account required.
- **R2** — *While* browsing a shared index, the app *shall* obtain rows only through the API, and
  *shall not* hold credentials for the index store.
- **R3** — *When* the user opens a local index file, the app *shall* query it in-browser without
  contacting a service.

## Notes

- Depends on: [`shared-index-api-contract.md`](shared-index-api-contract.md),
  [`shared-index-authentication.md`](shared-index-authentication.md),
  [`xdu-api-query-service.md`](xdu-api-query-service.md); S3 as an index target for the store (met, on
  `main` since #33).
- Related: [`access-scoped-queries.md`](access-scoped-queries.md) (umbrella).
- Found by: original roadmap; back-reference retrofitted 2026-09-07; revised 2026-09-11.
