---
status: unshaped
kind: fix
appetite: small
---

# The bundled DuckDB does not build against the macOS 27 SDK's libc++

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

xdu links DuckDB through the `duckdb` crate's `bundled` feature, which compiles the vendored C++
amalgamation from `libduckdb-sys` 1.4.4. With the macOS 27 command-line tools SDK, that compile
fails. The SDK's libc++ rejects the vendored thrift code, because thrift's `TEnumIterator` defines
no `operator==`. The error points into libc++'s `__tree` header (`__tree:1126`), not into DuckDB, so
it reads like a toolchain fault.

A host build works today only when `SDKROOT` points at a 26.x SDK. The macOS release legs and the
`macos-15` test leg still build because their runner images ship an older SDK. When those images
move to SDK 27, every macOS leg goes red, including the release build, with no change in this
repository.

## Why it was deferred

Found during the extension-distribution build, while running gates on a macOS 27 host. Fixing it was
outside that cycle: the GOAL there rules out moving the engine version or the `duckdb` crate, and
the defect sits in vendored upstream code rather than in xdu. It is **pre-existing**: `main` vendors
the same `libduckdb-sys` 1.4.4, so it meets the same error on that SDK. It has not reached CI yet;
the pull request's macOS legs passed on their current images.

## Outcome / vision

A macOS host with the current command-line tools builds xdu with no `SDKROOT` override, and a CI
runner image moving to SDK 27 does not break the macOS legs.

## Sketch of the acceptance criteria

- **R1** — WHEN xdu is built on macOS against the SDK 27 libc++, the bundled DuckDB SHALL compile
  with no `SDKROOT` override.
- **R2** — The fix SHALL NOT patch the vendored amalgamation in a way that changes the engine
  `source_id` the release scripts tie the shipped extensions to, unless it moves the engine version
  deliberately (and with it the pinned extensions).
- **R3** — Until the fix lands, the workaround (point `SDKROOT` at a 26.x SDK) SHALL be documented
  where a contributor building on macOS will find it.

## Notes

- Candidate routes, to weigh at shaping: a `duckdb` crate release whose vendored thrift defines the
  operator (the upstream fix), which moves the engine version and so re-pins the shipped extensions;
  a compiler flag that restores the older libc++ behaviour, if one exists for this check; or keeping
  the macOS legs pinned to an SDK 26 image, which only buys time.
- The release legs build natively on `macos-15` and `macos-15-intel`; watch their runner-image
  release notes for the SDK move.
- Related: [`duckdb-extension-distribution.md`](duckdb-extension-distribution.md) (pins the
  extensions to the engine version, so an engine bump re-pins them);
  [`ci-gates-are-advisory.md`](ci-gates-are-advisory.md).
- Found by: `duckdb-extension-distribution` build, recorded in P4.
