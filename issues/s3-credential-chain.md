---
status: unshaped
kind: feature
appetite: big
---

# S3 credentials beyond environment variables and the shared files

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

xdu resolves S3 credentials, region and endpoint in one place, `S3Settings::resolve` in
`src/s3.rs`, and both directions use it: the writer (`xdu -o s3://…`) and the readers (`xdu-find`,
`xdu-view`). It honours the `AWS_*` environment variables and the shared credentials and config
files, selected by `AWS_PROFILE` and located through `AWS_SHARED_CREDENTIALS_FILE` and
`AWS_CONFIG_FILE`, and nothing else; the module's header says so.

The rest of the standard AWS chain is missing: instance metadata (an EC2 role, ECS task roles), IAM
Identity Center (SSO) token caches, web identity (`AWS_WEB_IDENTITY_TOKEN_FILE` with
`AWS_ROLE_ARN`), `credential_process`, and role assumption (`role_arn` / `source_profile`). An
operator whose only credentials come from one of those has to export static keys first, which is
the practice those sources exist to avoid.

Two constraints shape any fix:

- The writer's client is built from the resolver's output only (`AmazonS3Builder::new()`, never
  `from_env()`), because `from_env()` reads more of the environment than the resolver allows and
  falls through to instance metadata on its own. Widening must happen in the resolver, so writer and
  readers keep resolving the same identity.
- The readers hand DuckDB static values, as the bound parameters of one prepared `CREATE SECRET`
  issued after the pinned `httpfs` is loaded. DuckDB's own `PROVIDER credential_chain` needs the
  separate `aws` extension, which is not in the pinned set. Adding it is a one-entry change to that
  set, but it would resolve credentials a second way, outside the Rust resolver.

## Why it was deferred

A GOAL non-goal of `s3-index-target`: the pilot honours the environment and the shared files only.
Each further source has its own refresh and expiry behaviour, and the instance-metadata and SSO
paths cannot be exercised by the loopback endpoint that pilot tests against. New with S3 support;
there was no S3 access before that cycle.

## Outcome / vision

On a host whose credentials come from any standard AWS source, `xdu`, `xdu-find` and `xdu-view` work
with no exported keys, and all three still resolve one identity through one resolver.

## Sketch of the acceptance criteria

- **R1** — WHEN no static credentials are set and a further source (to be chosen at shaping: instance
  metadata, SSO cache, web identity, `credential_process`, assumed role) supplies them, the tools
  SHALL use them, writer and readers alike.
- **R2** — The resolver SHALL stay the one source of credentials for both directions; the readers
  SHALL still pass them to DuckDB only as bound parameters, and no secret SHALL appear in SQL text
  or output.
- **R3** — Temporary credentials SHALL carry their session token to both directions, and an
  expiry during a long crawl SHALL either refresh or fail with a cause naming the source.
- **R4** — Whether to ship the `aws` extension (and `credential_chain`) instead of resolving in Rust
  SHALL be decided explicitly, on evidence, not by default.

## Notes

- Related: `spec/s3-index-target/` (GOAL non-goals and the credential clarification; research
  digest D2 on the bound `CREATE SECRET`, D3 on the writer's builder, D4 on the resolver);
  `spec/duckdb-extension-distribution/` for the pinned extension set.
- Found by: `s3-index-target` (GOAL, pilot split).
