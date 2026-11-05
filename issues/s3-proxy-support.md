---
status: unshaped
kind: feature
appetite: small
---

# S3 traffic through an HTTP proxy

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

Neither direction of xdu's S3 support uses a proxy. `HTTP_PROXY`, `HTTPS_PROXY` and `NO_PROXY` are
ignored, and the man pages of `xdu`, `xdu-find` and `xdu-view` say so. A site whose only route to
its object store is an HTTP proxy cannot build or read an index there.

The two directions use different HTTP stacks:

- The readers go through DuckDB's `httpfs`. The bundled engine (DuckDB 1.4.4) ignores the proxy
  environment variables; this was measured during `s3-index-target`'s research. `httpfs` has its own
  proxy settings (`http_proxy` and related), which xdu does not set.
- The writer goes through `object_store` on `reqwest`, which honours the proxy variables by default.
  xdu builds its client with `no_proxy()` on purpose (`S3Sink` in `src/s3.rs`).

If the writer alone honoured the variables, writer and readers could reach the store by different
routes behind a proxy, and a build that succeeds could be unreadable from the same host.

## Why it was deferred

That cycle's PLAN recorded it as deviation 6 (research digest D5): honouring the proxy in one
direction only is incoherent, and a shared proxy setting is new operator-facing surface the pilot
did not need. New with S3 support.

## Outcome / vision

One proxy configuration, from the standard variables or an explicit setting, reaches both the
writer and the readers, so that both take the same route to the store.

## Sketch of the acceptance criteria

- **R1** — WHEN a proxy is configured, the writer and the readers SHALL both send their S3
  requests through it, and both SHALL honour the same exclusions.
- **R2** — WHEN no proxy is configured, behaviour SHALL be unchanged: requests go straight to the
  endpoint.
- **R3** — Proxy credentials, if supported, SHALL reach DuckDB only as bound parameters and SHALL
  never appear in SQL text or output.
- **R4** — A test SHALL prove both directions used the proxy, against the loopback endpoint.

## Notes

- Related: `spec/s3-index-target/` (PLAN deviation 6, research digest D5); the man pages'
  statement that the proxy variables are not used, which changes with this.
- Found by: `s3-index-target` (PLAN).
