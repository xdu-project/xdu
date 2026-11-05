# GOAL — Ship DuckDB extensions instead of downloading them

> **Origin spec.** The *what* and *why* — the locked contract `xdu-review` grades against.
> The *how* lives in [`PLAN.md`](PLAN.md) and [`TECH.md`](TECH.md) (written by `xdu-plan`).
> Keep this at the right altitude: solved and bounded, but not over-specified — leave design
> freedom for the plan. Edit requirements here; do **not** silently drift them during build.

- **slug:** duckdb-extension-distribution
- **kind:** feature
- **appetite:** small  ·  *packaging and one shared loader; the spike already settled feasibility,
  so research is limited to closing its one open leg (R6). No reader changes behavior.*

## Problem

Every DuckDB capability xdu does not link statically arrives as a runtime download into
`$HOME/.duckdb/extensions/`, and on the hosts xdu targets that is a product defect, not a slow
first run. An air-gapped HPC login node is a normal deployment. `Cargo.toml:17-19` asks `duckdb` for
`parquet` alongside `bundled` precisely so the Parquet reader does not autoinstall a 12 MB extension,
and `tests/offline_tests.rs` holds that line by pointing `HOME` at an empty directory and asserting it
stays empty.

That escape hatch does not generalize. The `libduckdb-sys` 1.4.4 amalgamation (the version
`Cargo.lock` pins today) carries sources for exactly `core_functions`, `parquet` and `json`, so no
third extension can be statically linked whatever Cargo features are set. Anything else has to
arrive as a binary. The offline posture today is also an accident rather than a refusal: the bundled
build hard-codes autoinstall and autoload on, and xdu stays offline only because nothing it runs
triggers either.

The next roadmap entry, S3 as an index target, needs `httpfs` to read an index in place over ranged
requests, the native push-down path the spike measured at 0.13 MiB for a `--count` against an
80.1 MiB index. The full-text search evaluation is blocked on the same wall. Solving distribution once
unblocks both. The cost is a change to the release tarball layout, which `AGENTS.md` calls a contract,
which is why this gets its own cycle rather than riding on the S3 transport change.

## Outcome / vision

An operator installs xdu by any published channel, and the extensions it needs are already on disk,
pinned to the engine version the binaries were built against. Nothing downloads, nothing writes to
`$HOME`, and an air-gapped host behaves exactly like a connected one. When an extension is genuinely
absent, the tool names the file it looked for instead of reaching for the network. The S3 cycle that
follows then adds a transport, not an installer.

## Acceptance criteria (the contract)

- **R1** — Each release tarball SHALL carry the pinned DuckDB extension set, built for the same
  engine version and platform as the binaries beside it, and the release workflow SHALL fail rather
  than publish a tarball whose extension does not match on either count. The pinned set for this
  cycle is `httpfs` on every platform leg the release builds.
  [NEEDS CLARIFICATION: does the pinned set also include `aws` (for `PROVIDER credential_chain`),
  or only `httpfs`, leaving credential resolution to the S3 cycle in Rust?]
  [NEEDS CLARIFICATION: does every platform leg (Linux and macOS, both arches) ship the same set,
  or is the set per-target?]
- **R2** — The new tarball layout SHALL be recorded as the contract in `AGENTS.md`, and every
  channel that installs from or builds an image of xdu — `install.sh`, the HPCCM recipe, and the
  `Dockerfile` image — SHALL place the extensions where the installed tools find them with no
  further configuration.
- **R3** — WHEN a tool loads an extension, it SHALL load it from the pinned local location with
  autoinstall and autoload explicitly disabled, with no network access and no write under `$HOME`,
  and SHALL honor an operator override of that location for hosts that install elsewhere.
- **R4** — IF a required extension is absent from the location in effect, THEN the tool SHALL fail
  with a message naming the path it looked for and SHALL NOT attempt a download.
- **R5** — The local-index query path SHALL load no extension it does not link statically, and with
  autoinstall and autoload disabled the offline guarantee SHALL hold by refusal rather than by the
  absence of a trigger: `tests/offline_tests.rs` keeps passing with its cold `HOME` empty.
- **R6** — WHEN the release workflow builds a platform leg, it SHALL verify that the binaries in that
  leg's tarball, extracted, load the shipped extension with a cold `HOME` and autoinstall disabled.
  This closes the spike's one inferred claim (the `linux_amd64` glibc floor was read from symbols,
  never exercised in a manylinux build) on every leg rather than one.
  [NEEDS CLARIFICATION: with no S3 consumer yet, should this cycle add any operator-visible surface
  for extension loading (a diagnostic naming the location in effect), or stay library-only, verified
  by tests and this release check?]

## Non-goals (no-gos)

- Reading or writing an index over S3, S3 credential resolution, and any `s3://` locator parsing —
  the next cycle, `issues/s3-index-target.md`. This cycle ships the binary that cycle loads.
- Full-text search and the `fts` extension — `issues/duckdb-fts-evaluation.md`. The mechanism must
  admit another entry in the pinned set; adding one is not this cycle.
- Any runtime download path, including an opt-in one. An absent extension is an error (R4).
- Building extensions from source, or patching the vendored amalgamation to link `httpfs` statically.
- Bumping the `duckdb` crate or moving the engine version.
- Native OS packages (`.deb`/`.rpm`) — `issues/native-os-packages-deb-rpm.md`, which inherits the
  layout this cycle sets.
- New platform legs, or changing the manylinux_2_28 glibc floor the release baseline set.
- Any change to the index format, the Parquet schema, or reader query behavior.

## Clarifications

- **Q:** Does the pinned set include `aws` for `PROVIDER credential_chain`, or only `httpfs`?
  — **A:** [NEEDS CLARIFICATION: open. The seed defers the trade to the S3 cycle but notes it decides
  how many binaries ship here; the spike (claim 11) showed credentials can reach DuckDB through the
  crate's `Config` API without SQL or `aws`.]
- **Q:** Does every platform leg ship the same set, or is it per-target? `httpfs` alone is 24 MiB
  raw, 8.8 MiB gzipped for `linux_amd64`. — **A:** [NEEDS CLARIFICATION: open; the seed flags it as a
  promotion question.]
- **Q:** Library-only, or an operator-visible diagnostic for extension loading, given no CLI
  consumer lands this cycle? — **A:** [NEEDS CLARIFICATION: open.]

## Related materials

- Seed: `issues/duckdb-extension-distribution.md` (pre-shaped Problem, draft R-IDs as input).
- ROADMAP entry: `ROADMAP.md` ("Ship DuckDB extensions instead of downloading them").
- Evidence: `research/duckdb-native-s3-reads.md` — claims 1–5 (redirected extension directory,
  offline `LOAD`, load by absolute path, clean failure with autoinstall off) and 13 (glibc floor,
  inferred); probes under `bench/spikes/`.
- First consumer: `issues/s3-index-target.md`. Second: `issues/duckdb-fts-evaluation.md`.
- Anchors: `Cargo.toml:17-19` (`bundled` + `parquet`), `tests/offline_tests.rs` (cold-`HOME`
  guard), `AGENTS.md` § release tarball layout, `.github/workflows/release.yaml` (four platform
  legs, Linux built in manylinux_2_28), `install.sh`, `hpccm/xdu.py` (extracts the tarball into its
  prefix), `Dockerfile`.
