# 01 — Closing R6: how each release leg gets, checks and loads `httpfs`

> Plan-step research, 2026-11-04. Scope is the GOAL's one open leg (R6) plus the release-side
> unknowns it drags in. Everything here was observed on the planning host (darwin/arm64) against the
> live extension repository and the published `duckdb`/`libduckdb-sys` 1.4.4 crate sources, unless
> marked *inferred*.

## Where the binary comes from

`https://extensions.duckdb.org/v1.4.4/{platform}/httpfs.duckdb_extension.gz` answers 200 for all four
platforms xdu ships, and 404 for `linux_amd64_gcc4` (that suffix is gone in 1.4):

| platform | gz bytes | raw bytes | footer `extension_version` |
|---|---|---|---|
| `linux_amd64` | 9,185,059 | 25,044,926 | `13f8a81` |
| `linux_arm64` | 8,209,318 | 22,846,798 | `13f8a81` |
| `osx_amd64` | 4,322,700 | 12,179,142 | `13f8a81` |
| `osx_arm64` | 3,939,803 | 11,418,014 | `13f8a81` |

All four objects carry `last-modified: 2026-01-22`. Upstream can republish a core extension for the
same engine version (the `extension_version` field moves), so a URL is pinned to an engine version
and platform, not to bytes. The GOAL pins exactly those two counts.

## The footer is the static match check

Every file ends in 512 bytes: 256 of metadata, then a 256-byte signature. The metadata is eight
32-byte NUL-padded fields, stored last-field-first. Read from the end of the file:

| `tail -c N \| head -c 32` | field | observed |
|---|---|---|
| `288` | metadata magic | `4` |
| `320` | platform | `linux_amd64` … |
| `352` | DuckDB version | `v1.4.4` |
| `384` | extension version | `13f8a81` |
| `416` | ABI type | `CPP` |

POSIX `tail`/`head`/`tr -d '\000'` read these on both GNU and BSD userlands, so a release leg can
compare the footer against the engine without a Python or DuckDB CLI dependency. Checking the magic
field first is what keeps the offsets honest: a future footer layout fails on the magic instead of
comparing garbage that happens to differ.

The `CPP` ABI is the strict one: DuckDB refuses to `LOAD` a C++-ABI extension whose version field is
not its own, and refuses a platform mismatch, unless `allow_extensions_metadata_mismatch` is set,
which xdu never does. So a mismatched file fails twice, statically in the footer read and dynamically
at load.

## What the extension needs from the host

- **No DuckDB symbols from the executable.** `objdump -T` on `linux_amd64` lists 521 undefined
  dynamic symbols and none is in the `duckdb::` namespace; `nm -u` on `osx_arm64` likewise. The
  extension carries its own copy of the engine code it uses, so the release profile's `strip = true`
  and LTO cannot break `LOAD` by dropping exports. No `-rdynamic` is needed.
- **Linux runtime floor.** `NEEDED` is `libpthread`, `libdl`, `librt`, `libstdc++.so.6`, `libm`,
  `libgcc_s`, `libc`, `ld-linux-*`. Version nodes top out at `GLIBC_2.28`, `GLIBCXX_3.4.22`,
  `CXXABI_1.3.11`, `GCC_4.5.0`, all inside the manylinux_2_28 / RHEL8 baseline (`GLIBCXX_3.4.25`,
  `GCC_8.0.0`). Spike claim 13 read the same thing off symbols; the R6 load check is what turns it
  into an observation.
- **macOS.** Both Mach-O builds target `minos 11.0` and link only system libraries and frameworks.
- **Two C++ runtimes on `linux_amd64`** (*inferred risk*). `.cargo/config.toml` passes
  `-static-libstdc++` for `x86_64-unknown-linux-gnu` only, while the extension binds
  `libstdc++.so.6` dynamically. The release workflow's own comment records that the static link left
  `NEEDED libstdc++` in place, so the host may run partly on a static copy. Objects and exceptions
  cross the engine/extension boundary, so a bare `LOAD` succeeding does not prove the pair works.
  The check therefore also runs one query through `httpfs` that fails inside the extension (a refused
  loopback connection) and requires a clean SQL error, not an abort.

## Which process can do the loading

None of the four shipped binaries loads an extension this cycle. The loader is library-only (GOAL,
Clarification 3), and R5 forbids the local-index query path from loading one. A shipped binary can
only be made to load `httpfs` through a trigger, and every available trigger is either an
operator-visible diagnostic (a flag or subcommand, ruled out by the non-goals and by invariant §10's
man-page obligation) or a reader path that loads a non-static extension (ruled out by R5).

What can run is a probe built in **the same `cargo build` invocation** as the tarball binaries, from
the same lockfile, target and profile, that calls the production loader. Copied into a scratch
extraction of the tarball's `bin/`, it resolves the extension directory exactly as an installed tool
would. Two cheap checks tie its evidence to the shipped binaries. The engine's `source_id` (from
`pragma_version()`) must appear in each DuckDB-linking binary in the tarball, and the shipped
`xdu` and `xdu-find` must answer a query from the extracted tree under the same cold `HOME`. Whether
the `source_id` string survives `strip` plus LTO is *inferred*: it is reachable from the
`version()` builtin. P2 confirms it on the host before the workflow relies on it.

*Resolved at the plan gate, 2026-11-05:* GOAL R6 was amended to exactly this reading (Clarification
4). The extension loads into a same-build engine run from the extracted layout, and the shipped
binaries run separately under the same cold `HOME`.

A Cargo **example** is the right home for the probe rather than a `[[bin]]`: `cargo install`
installs every bin target (it already installs `gen-completions`), but never an example.

## Runner availability (found while checking the macOS legs)

The GitHub runner-images README states: "All macOS 14 images retired from GitHub Actions on
November 2, 2026." `release.yaml` runs both Apple legs on `macos-14`, and `test.yaml` runs its macOS
test host there. As of today those jobs have no runner. Current labels: `macos-15` (arm64) and
`macos-15-intel` (x64). Moving `x86_64-apple-darwin` to `macos-15-intel` also makes that leg native,
so its load check runs without Rosetta. Whether the arm64 images carry Rosetta could not be
confirmed from the image READMEs, so the plan does not depend on it.

## Engine facts the loader relies on

- `libduckdb-sys` 1.4.4 `build.rs` defines `DUCKDB_EXTENSION_AUTOINSTALL_DEFAULT=1` and
  `..._AUTOLOAD_DEFAULT=1`, and always links `core_functions`; `parquet` and `json` follow the
  Cargo features (xdu enables only `parquet`).
- `duckdb::Config::enable_autoload_extension(false)` sets both `autoinstall_known_extensions` and
  `autoload_known_extensions` to `0`; `Connection::open_in_memory_with_flags(config)` applies it at
  open. `allow_unsigned_extensions` defaults off, so the official signature is checked at `LOAD`.
- `json` is in the amalgamation but not linked, so `'{}'::JSON` is a local autoload trigger that
  needs no `httpfs` and no remote file. It is what the refusal test uses.

## Not verified here

No container runtime exists on the planning host, so nothing was executed on Linux. The two Linux
legs' load checks first run in CI, on the pull request (see PLAN §5).
