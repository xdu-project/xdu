---
slug: s3-index-target
title: "Build an index into S3 and query it in place"
kind: feature
appetite: big
status: in_progress
branch: feature/s3-index-target
base: main
current_phase: P1
last_updated: "2026-11-05"
phases:
  - id: P1
    name: "Loopback endpoint, resolver, S3 connection; xdu-rm refuses s3://"
    status: pending
    satisfies: [R5, R7, R8, R10]
    depends_on: []
    parallel: false
    hammerable: false
    hill: uphill
    verify: 'set -eu; sh spec/s3-index-target/verify/wire.sh harness_ 5; cargo test --locked --lib s3:: >target/p1-lib.log 2>&1; grep -qE "test result: ok. [1-9][0-9]* passed" target/p1-lib.log; cargo test --locked --test rm_tests --test offline_tests --test find_tests --test s3_tests; if grep -rnE "let _ = .*load_extension|load_extension\(.*\)\.ok\(\)" src; then exit 1; fi; grep -qF "include-ignored" .github/workflows/test.yaml; grep -qF "fetch.sh" .github/workflows/test.yaml; cargo clippy --locked --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; echo P1-GATE-GREEN'
  - id: P2
    name: "xdu-find queries an s3:// index in place (demo path)"
    status: pending
    satisfies: [R2, R3, R4, R6, R9]
    depends_on: [P1]
    parallel: false
    hammerable: false
    hill: uphill
    verify: 'set -eu; sh spec/s3-index-target/verify/wire.sh find_ 14; cargo test --locked --lib; cargo test --locked --test find_tests --test offline_tests --test version_tests --test s3_tests; .agents/factory/bin/temp_index.sh sh -c "xdu-find -i \"\$XDU_INDEX\" --count"; grep -qF "XDU_EXTENSION_DIR" doc/xdu-find.1.scd; grep -qF "AWS_ENDPOINT_URL_S3" doc/xdu-find.1.scd; grep -A20 "check share/man/man1/xdu-find.1" .github/workflows/test.yaml | grep -qF "XDU_EXTENSION_DIR"; grep -qF "s3://" README.md; cargo clippy --locked --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; echo P2-GATE-GREEN'
  - id: P3
    name: "xdu -o s3:// builds the index into the bucket"
    status: pending
    satisfies: [R1, R4, R5, R9]
    depends_on: [P2]
    parallel: false
    hammerable: false
    hill: uphill
    verify: 'set -eu; sh spec/s3-index-target/verify/wire.sh write_ 10; cargo test --locked; .agents/factory/bin/temp_index.sh sh -c "xdu-find -i \"\$XDU_INDEX\" --count"; grep -qF "AWS_PROFILE" doc/xdu.1.scd; grep -A20 "check share/man/man1/xdu.1" .github/workflows/test.yaml | grep -qF "AWS_PROFILE"; for c in openssl-sys native-tls aws-lc-sys; do if cargo tree --locked -e normal -i $c >/dev/null 2>&1; then echo "$c is in the runtime tree" >&2; exit 1; fi; done; cargo tree --locked -e normal -i ring >/dev/null; cargo clippy --locked --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; echo P3-GATE-GREEN'
  - id: P4
    name: "xdu-view browses an s3:// index in place"
    status: pending
    satisfies: [R2, R6, R9]
    depends_on: [P3]
    parallel: false
    hammerable: true
    hill: uphill
    verify: 'set -eu; sh spec/s3-index-target/verify/wire.sh view_ 5; cargo test --locked; grep -qF "XDU_EXTENSION_DIR" doc/xdu-view.1.scd; grep -qF "AWS_ENDPOINT_URL_S3" doc/xdu-view.1.scd; grep -A20 "check share/man/man1/xdu-view.1" .github/workflows/test.yaml | grep -qF "XDU_EXTENSION_DIR"; cargo clippy --locked --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; echo P4-GATE-GREEN'
  - id: P5
    name: "Close the cycle: full S3 suite, gate literals, deferral ledger"
    status: pending
    satisfies: [R9, R10]
    depends_on: [P4]
    parallel: false
    hammerable: false
    hill: uphill
    verify: 'set -eu; for f in s3-index-rm s3-credential-chain local-reader-bound-parameters s3-proxy-support; do test -s issues/$f.md; grep -qF "## Problem" issues/$f.md; grep -qF "issues/$f.md" ROADMAP.md; done; sh spec/s3-index-target/verify/wire.sh "" 34; cargo test --locked; grep -qF "include-ignored" .github/workflows/test.yaml; cargo clippy --locked --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; echo P5-GATE-GREEN'
review:
  last_reviewed_commit: ""
  verdict: none
  blocked_reason: ""
  cycle: 0
---

# TECH.md — Build an index into S3 and query it in place

The **context engine and finite-state machine** for building this feature. The YAML
frontmatter above is the resume ground-truth (read it with
`uv run --with pyyaml python .agents/factory/bin/next_phase.py spec/s3-index-target/TECH.md`); the
per-phase checklists below are the work. `xdu-build` executes the next actionable phase, runs its
`verify:` command, updates state via
`uv run --with pyyaml python .agents/factory/bin/set_phase.py …`, and makes one atomic code+state
commit.

- **Vision / requirements (locked):** [`GOAL.md`](GOAL.md) — R-IDs are the contract.
- **Authoritative design:** [`PLAN.md`](PLAN.md) (§2.x below refers to it).
- **Backing research:** [`research/00-digest.md`](research/00-digest.md) (D1–D10) + briefs
  `01`–`05`.

## Frontmatter field reference

See [`.agents/factory/templates/TECH.md`](../../.agents/factory/templates/TECH.md). Nothing here
departs from it.

## Conventions (apply to every phase)

- Commit conventions, code style, and load-bearing invariants come from
  [`AGENTS.md`](../../AGENTS.md) — it is the constitution. Consult
  [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) §2, §2b, §3, §5, §7, §10,
  §11, §13 for this change.
- One phase per `xdu-build` invocation; one atomic commit containing **both** the code and the
  `TECH.md` state change, subject `[feature] Build s3-index-target P<n>: …`. No `Co-Authored-By`.
- A CLI/feature change updates the affected `doc/*.scd` **in the same commit**, with the CI
  literal list in `.github/workflows/test.yaml` and `AGENTS.md`'s duplicated-literal sentence moved
  with it (§13). Read each page through `scdoc < f.scd | mandoc -Tutf8 | col -b`, never exit 0
  alone; scdoc is not installed in every agent environment, so say so if it was not rendered.
- **Shared verify prerequisite.** The wire tests need the pinned httpfs. Once per worktree:
  `cargo build --locked --example extension-probe` then
  `sh packaging/extensions/fetch.sh target/debug/examples/extension-probe target/test-ext`
  (network to the DuckDB extension host). **Never** fetch into `target/lib/xdu/extensions`: that is
  the debug binaries' default lookup and would hide a reader that wrongly loads httpfs for a local
  index. The wire gate [`verify/wire.sh`](verify/wire.sh) (written at plan time, not by a build
  phase) defaults `XDU_TEST_EXTENSION_DIR` to `target/test-ext`, fails if the file is absent or sits
  at the default lookup, and passes only when the passed count equals the `--list` count for the
  phase's prefix and reaches its minimum. Under `--include-ignored` libtest always prints
  `0 ignored`, so that phrase is never the gate
  ([`research/06`](research/06-adversarial-review.md)).
- **Test naming.** All wire tests live in `tests/s3_tests.rs`, are `#[ignore = "needs the pinned
  httpfs; set XDU_TEST_EXTENSION_DIR and run with --include-ignored (CI does)"]`, and carry a
  per-phase prefix (`harness_`, `find_`, `write_`, `view_`) so each `verify:` selects its own. Names
  describe behaviour; no spec ids in source (§13). Test functions return `()` and never return
  early: `TestS3::start()` **panics** when it cannot bind or find the extension, whether or not
  `XDU_TEST_S3_REQUIRED` is set (the `#[ignore]` is what keeps an unequipped machine quiet).
  Helpers live in `tests/common/`, where `wire.sh`'s early-return check does not look.
- **Hermetic children.** Every spawned binary in an S3 test runs with all `AWS_*`, `DUCKDB_*` and
  `*_PROXY` variables removed, fixture credential/config files, and an empty `HOME`, `TMPDIR` and
  working directory that are asserted still empty afterwards (readers and writer alike: no staging,
  no persistent secret, no stray local write). The secret string is asserted absent from stdout and
  stderr wherever a test has both in hand.
- **Every wire test goes through the counting proxy**, and every one asserts that no GET of a
  `.parquet` key covered the whole object (open-ended `bytes=a-` and suffix `bytes=-n` ranges count
  as whole). Endpoints in the fixture's config name only the proxy, so traffic cannot bypass it.
- **Empty `XDU_EXTENSION_DIR`** in a test means an empty *directory*: an empty *value* is treated
  as unset and falls back to the default lookup.
- **Run A / run B (binding, foreman at the PLAN gate).** Some claims can only be proved in CI:
  httpfs loaded by a debug build on linux_amd64, the release legs (manylinux_2_28 glibc floor,
  NEEDED libraries, `extension-check`), and the wire tests on both test legs. So every phase runs
  twice. **Run A** does the checklist, passes the phase's local `verify:`, commits code and state
  with the phase left `in_progress`, and hands back. The foreman pushes. **Run B** reads the CI
  logs of that push, quotes the evidence lines named in the phase's *Run B evidence* bullet into
  `spec/s3-index-target/evidence/p<n>-ci.md` (run id, job, step, the line verbatim), re-runs the
  local `verify:`, marks the phase `done` and advances. A missing or contradicting line is a phase
  failure: run B stops and reports; it never re-words the evidence.
- **Dependency floor (binding, foreman at the PLAN gate).** No `openssl-sys`, `native-tls` or
  `aws-lc-sys` in the runtime dependency tree; P3's `verify:` enforces it, and it stays a gate. If
  ring fails in a release leg, the fallback (hand-rolled SigV4 over `ureq`) goes back to the
  foreman as a GOAL/PLAN question. Never swap the transport, the TLS provider or the gate silently.
- **No `.partial`, no staging, no cache.** Any code path that writes an index object to local disk
  for an `s3://` locator fails the phase.

---

## Phase P1 — Loopback endpoint, resolver, S3 connection; xdu-rm refuses s3://
**Satisfies:** R5, R7, R8, R10 · **Depends on:** — · **Size:** ~0.5 day
**Goal:** the foundation every later phase stands on, proven end to end: a test can start an
SigV4-checking S3 endpoint on loopback, resolve credentials the way R5 says, open a DuckDB
connection that loads the pinned httpfs and holds those credentials only as bound parameters, and
read an object through the counting proxy. CI runs it on both legs. `xdu-rm` refuses `s3://`.

- [ ] `src/s3.rs` (new, re-exported via `lib`): `IndexLocator`/`S3Locator` parse on `OsStr`
  (PLAN §2.1) with unit tests: `s3://b/p`, trailing `/`, empty bucket, userinfo, glob
  metacharacters, uppercase `S3://` stays local, non-UTF-8 stays local.
- [ ] `src/s3.rs`: `S3Settings::resolve` over an `EnvSource` trait (PLAN §2.2, digest D4),
  hand-rolled INI, redacted `Debug`. Unit tests from fixture files: env beats profile; credentials
  file beats config; `AWS_PROFILE` selects `[profile x]`/`[x]`; `AWS_REGION` > `AWS_DEFAULT_REGION`
  > profile > `us-east-1`; `AWS_ENDPOINT_URL_S3` > `AWS_ENDPOINT_URL` > `services` > `endpoint_url`;
  path style iff an endpoint is set; half-set env credentials, no credentials, endpoint with a path
  → named errors that contain no secret.
- [ ] `src/engine.rs`: `open_s3_connection(&S3Settings)` = `open_connection` + `load_extension
  ("httpfs")` + one prepared `CREATE SECRET … ?` (PLAN §2.3). Amend the absent-extension text to
  name the path and both remedies (copy `lib/xdu/extensions/` from a release tarball; set
  `XDU_EXTENSION_DIR`), keeping `assert_names_absent_file`'s substrings. `open_connection` stays
  byte-identical.
- [ ] `Cargo.toml` dev-dependencies: `s3s = "=0.17.0"`, `s3s-fs = "=0.17.0"`, `hyper-util`, `tokio`
  (digest D6). Confirm the toolchain pin builds them.
- [ ] `tests/common/s3.rs`: `TestS3` (in-process s3s-fs on `127.0.0.1:0`, `SimpleAuth`, temp root,
  `seed_index(local_index, prefix)` by copying into the root), the std-only counting proxy (passes
  `Host` through; forces `Connection: close` on both legs, or parses every request on a kept-alive
  connection, and asserts request count = connection count in the first case; records per request
  method, path, `Range`, response status, `Content-Range` and bytes to client) and the hermetic
  child `Command` builder. A refused bind or a missing extension panics, always.
- [ ] `tests/s3_tests.rs` `harness_` tests: an `open_s3_connection` reads a seeded object through
  the proxy and the proxy saw it; a wrong secret is refused with a 403-class error that does not
  contain the secret; a stray `DUCKDB_S3_ENDPOINT` in the environment does not redirect the
  connection (the bound secret wins, digest D2). Per amended R5/R6, on the connection
  `open_s3_connection` returns: `SELECT secret_string FROM duckdb_secrets()` contains
  `secret=redacted` and not the secret; no row of `duckdb_settings()` contains the secret (or the
  session token); and every SQL string `open_s3_connection` builds is exposed to a unit test
  (a pure `fn` returning the statement text) that asserts neither secret value occurs in it.
- [ ] Always-run tests: `xdu-find -i s3://…` with `XDU_EXTENSION_DIR` set to an empty directory
  fails naming that path and both remedies — assert the phrase "release tarball" and
  `XDU_EXTENSION_DIR`, not `lib/xdu/extensions` (the default path already contains it). With a
  garbage `httpfs.duckdb_extension` in `XDU_EXTENSION_DIR`, local `xdu-find`, `xdu-rm --dry-run`
  and `xdu -o <local dir>` answer correctly **with empty stderr** (a swallowed load error would
  print or would have to be discarded, which the verify grep refuses). `offline_tests.rs` untouched
  and green.
  (The xdu-find S3 branch that makes the first test reachable is the minimal prologue of P2's
  §2.6 — parse, resolve, `open_s3_connection`, then `bail!` "not yet" is acceptable here only if
  P2 replaces it in the same cycle; prefer landing the real prologue.)
- [ ] `src/bin/xdu-rm.rs`: refuse an `s3://` locator right after parse with "S3 indices are not yet
  supported by xdu-rm", exit non-zero (PLAN §2.7); `tests/rm_tests.rs` covers `-i` and `XDU_INDEX`
  and asserts nothing else ran (no extension load, no pool). `doc/xdu-rm.1.scd`: one sentence
  under `-i`; re-count `XDU_INDEX` if the sentence names it.
- [ ] `.github/workflows/test.yaml`, both legs: build `extension-probe`, `fetch.sh` into
  `$RUNNER_TEMP/xdu-ext`, then an "S3 tests" step after Test:
  `XDU_TEST_EXTENSION_DIR=… sh spec/s3-index-target/verify/wire.sh "" <min>`, the one guard script
  both verify and CI run (cargo's exit status read directly; passed count equal to the
  `--list --include-ignored` count). Raise `<min>` in the same commit each later phase adds tests.
  The step's last line, `s3-wire: '' N of N passed`, is what each run B reads.
- **Run B evidence:** in the test workflow, jobs `Test (ubuntu-24.04)` and `Test (macos-15)`,
  step "S3 tests": `s3-wire: '' N of N passed` with N ≥ 5 (the ubuntu line is the first observed
  httpfs load by a debug build on linux_amd64). In the release workflow (it runs because this phase
  touches `Cargo.toml` and `src/engine.rs`), jobs `Build (x86_64-unknown-linux-gnu)` and
  `Build (aarch64-unknown-linux-gnu)`: `extension-check: ok platform=linux_amd64 engine=v1.4.4`
  and `extension-check: ok platform=linux_arm64 engine=v1.4.4`.
- **Verify:** see frontmatter (lib `s3::` unit tests, always-run suites, `harness_` wire tests with
  `verify/wire.sh`, no discarded `load_extension` result, the CI step present, clippy, fmt).
- **Touches:** `src/s3.rs`, `src/lib.rs`, `src/engine.rs`, `src/bin/xdu-find.rs` (prologue only),
  `src/bin/xdu-rm.rs`, `Cargo.toml`, `Cargo.lock`, `tests/common/`, `tests/s3_tests.rs`,
  `tests/rm_tests.rs`, `doc/xdu-rm.1.scd`, `.github/workflows/test.yaml`.

## Phase P2 — xdu-find queries an s3:// index in place (demo path)
**Satisfies:** R2, R3, R4, R6, R9 · **Depends on:** P1 · **Size:** ~0.75 day
**Goal:** `xdu-find -i s3://bucket/prefix` (or `XDU_INDEX`) returns what it returns locally, pays
for footers and selected row groups only, refuses bad markers and bad endpoints with a named cause,
and binds everything S3 sends to SQL. The demo works after this phase.

- [ ] `src/lib.rs`: `index_glob_s3` beside `index_glob`; `parquet_source(&IndexLocator, partition,
  filename) -> (String, Vec<String>)` with a unit test pinning the local strings byte for byte
  (PLAN §2.4).
- [ ] `src/lib.rs`: `read_completion_marker_s3` (glob, then size, then content; cap before any
  GET), `classify_s3_error`, and gates taking a pre-read `MarkerRead` (PLAN §2.5). Local marker
  path keeps its FIFO-safe read.
- [ ] `src/bin/xdu-find.rs`: full S3 prologue (PLAN §2.6); all 7 statements through
  `parquet_source` with `params_from_iter`. Local behaviour unchanged.
- [ ] Wire tests (`find_` prefix):
  - parity local vs S3 for: default listing, `--count`, `--top N`, each sort, `-u` (incl.
    `__root__`), size/age/pattern filters, csv and json output — stdout compared byte for byte;
  - R3 fixture: about 8 chunks of about 2 MiB (≈16 MiB) over 4 partitions, encoded by xdu's own
    encoder — expose the chunk encoder from `src/crawl.rs` as one lib function (same
    `WriterProperties`) that P3's `PartitionBuffer` then calls — with incompressible paths, plus a
    real marker. The test asserts its own floor at run time: ≥10 MiB of `*.parquet` under
    `<bucket>/<prefix>/`, ≥4 partitions, ≤12 chunks;
  - R3 measure, `--count` over all partitions through the proxy: the set of keys fetched with a
    bounded `Range` (206, `Content-Range` present, length < object) **equals** the set of chunk
    keys; no unranged or open-ended GET of any `.parquet` key; bytes to client (headers and LIST
    included) < 5% of the summed `*.parquet` sizes under `<bucket>/<prefix>/` (marker and any
    endpoint sidecar files excluded). Print the measured ratio;
  - R3 negative control: the binary itself, through the same proxy and fixture, with a `-p`
    pattern that matches nothing (forcing a scan of the path column) receives > 25%. This proves
    the meter sees chunk bytes; no in-test connection stands in for the binary;
  - marker cap: stderr names the size cap; the proxy saw a HEAD of the exact marker key and no GET
    of it; boundary pair — a marker of exactly `MARKER_READ_LIMIT` bytes is read, one byte more
    is refused;
  - R4: dead endpoint (`127.0.0.1:1`, never a released ephemeral port another test may reuse),
    wrong secret, no credentials, missing bucket, missing prefix, missing marker, unknown
    `format` → non-zero, empty stdout, secret absent, and a **distinct** cause string per case,
    so a classifier that maps everything to one message fails;
  - R6: a prefix containing `'` (the parser allows it) answers the same as the local run, which
    `format!` into SQL could not;
  - credentials via env and via `AWS_PROFILE` with `AWS_SHARED_CREDENTIALS_FILE`/`AWS_CONFIG_FILE`
    fixtures, endpoint via `AWS_ENDPOINT_URL` and via a profile `services` section.
- [ ] Probe while building (record the result in the commit body): `read_parquet(?, filename=true)`
  and a bound glob inside a CTE both bind (`01` §2 left them unprobed).
- [ ] `doc/xdu-find.1.scd`: `s3://BUCKET/PREFIX` under `-i`; a paragraph on S3 indices
  (ranged reads, nothing copied locally, proxy variables not used, `xdu-rm` refuses); `ENVIRONMENT`
  gains the ten `AWS_*` names and `XDU_EXTENSION_DIR` with the `cargo install` remedy. Add the
  page's literals to the CI check with counts derived from the rendered page (`05` §3 hazards:
  `AWS_ENDPOINT_URL` counts include `AWS_ENDPOINT_URL_S3`; an example naming `XDU_INDEX` bumps its
  count). `AGENTS.md:277`'s sentence names `XDU_EXTENSION_DIR`.
- [ ] `README.md`: `### Indexes in S3` (query half) before `### Querying with DuckDB`; one clause at
  `:99-101` that S3 indices need `httpfs`.
- [ ] `AGENTS.md` and `invariants.md` §5: S3 locators and partition names are bound; local raw
  `format!` note stands; `open_s3_connection`'s `CREATE SECRET` joins the engine surface.
- **Run B evidence:** jobs `Test (ubuntu-24.04)` and `Test (macos-15)`, step "S3 tests":
  `s3-wire: '' N of N passed` with N ≥ 19; job `Packaging (man + completions generate)`, step
  "Assert critical literals survive into the published man-page text", green with the new
  `xdu-find.1` checks; the R3 test's printed ratio line from the ubuntu leg.
- **Verify:** see frontmatter.
- **Touches:** `src/lib.rs`, `src/s3.rs`, `src/bin/xdu-find.rs`, `tests/`, `doc/xdu-find.1.scd`,
  `README.md`, `AGENTS.md`, `.agents/factory/invariants.md`, `.github/workflows/test.yaml`.

## Phase P3 — xdu -o s3:// builds the index into the bucket
**Satisfies:** R1, R4, R5, R9 · **Depends on:** P2 · **Size:** ~1 day
**Goal:** `xdu <src> -o s3://bucket/prefix` writes the index a local run writes, chunk by chunk,
with §2b's ordering, and a re-run leaves no stale chunk. Readers from P2 answer from it.

- [ ] `Cargo.toml`: `object_store = { version = "0.14.2", default-features = false, features =
  ["aws-base", "reqwest", "ring"] }`, `reqwest = { version = "0.13", default-features = false,
  features = ["rustls-no-provider"] }`, `rustls = { version = "0.23", default-features = false,
  features = ["ring", "std"] }`, `tokio` (`rt`, `rt-multi-thread`), `futures`, `bytes` (digest D3).
  Install the ring provider once before any client.
- [ ] `src/crawl.rs`: `IndexSink`, `LocalSink` (byte-for-byte today's files, rename and first-gap
  prune), `PartitionBuffer` encodes into a `Vec<u8>` passed by value (PLAN §2.8, port from
  `32ba641` with digest D9's changes). Existing `crawl_tests` stay green unchanged.
- [ ] `src/s3.rs`: `S3Sink` (owned runtime; `AmazonS3Builder::new()` from resolver output only;
  `no_proxy()` reqwest connector; bounded retry and timeouts; PUT at the final key; LIST + DELETE
  of every higher id; marker DELETE/PUT; key round-trip check for partition names).
- [ ] `src/bin/xdu.rs`: `-o` stays `PathBuf`; classify on `OsStr`; the S3 sequence of PLAN §2.8 with
  the marker DELETE after every rejecting check; local sequence untouched. Per amended R1, refuse
  a top-level name that is not UTF-8 or not safe as an S3 key segment and a DuckDB glob before the
  clear, exiting non-zero and naming the directory; per amended R4, an absent or empty prefix is a
  new index, while a missing bucket, an unreachable endpoint or rejected credentials fail.
- [ ] Wire tests (`write_` prefix):
  - round trip: `xdu -o s3://` vs a local run of the same tree — key set equals file set;
    `__root__` present; per partition, an in-test DuckDB `SELECT * … ORDER BY path` over each tree
    returns equal rows in **all eight columns**; the Parquet schema, key-value metadata and
    `created_by` match (a `COPY` re-encode or a mangled column fails); marker body equal minus
    `completed_at`;
  - the writer child runs with empty `HOME`, `TMPDIR` and working directory, all three still empty
    afterwards, and the source tree's listing unchanged (catches a misclassified `./s3:/…` write);
  - re-run with fewer files leaves no stale chunk in any partition it wrote; the test first asserts
    the earlier run wrote more than one chunk in that partition;
  - ordering, through the proxy: a successful run issues LIST, then DELETE of the marker, then the
    chunk PUTs, and the marker PUT last;
  - rejections **with valid credentials** (missing source, a reserved partition name, a partition
    name that would not round-trip, per R1): non-zero, the proxy saw **no DELETE**, and the existing
    marker's bytes and chunks are unchanged. Wrong secret / dead endpoint / missing bucket: the same
    assertions (these alone cannot catch an early clear, because the DELETE would fail too);
  - read-only credentials (a second identity without write, or a test-only `S3` wrapper around
    s3s-fs that refuses writes if s3s-fs cannot express it): the marker is still in place;
  - a failure injected after the clear (e.g. a source subtree made unreadable without
    `--allow-errors`): non-zero, no marker, and at least one chunk PUT seen before the failure
    (chunks go up as they finalize, not at the end).
- [ ] Release-build check: the runtime dependency tree holds no `openssl-sys`, `native-tls` or
  `aws-lc-sys` (`cargo tree -e normal -i …`, in `verify:`), and does hold `ring`. Record in the
  commit body that the manylinux_2_28 leg's glibc/NEEDED check must pass on the first push (ring is
  proven there only as a build dependency).
- [ ] `doc/xdu.1.scd`: `-o` accepts `s3://BUCKET/PREFIX`; S3 paragraph (chunks sent as they
  finalize, nothing assembled locally, proxy variables not used); `ENVIRONMENT` gains the ten
  `AWS_*` names; CI literal counts. `README.md`: the build half of `### Indexes in S3`.
- [ ] `invariants.md` §2/§7 and the matching `AGENTS.md` text: S3 finalize = one PUT at the final
  key plus LIST prune; the runtime's threads in the budget.
- **Run B evidence:** test legs as above with N ≥ 29. Release workflow, both `Build (…-linux-gnu)`
  jobs: step "Assert glibc floor" prints `… needs max GLIBC_2.x` lines with x ≤ 28 and no
  `floor breach`, `runtime breach` or `unexpected dynamic needs` line; then
  `extension-check: ok platform=linux_amd64 engine=v1.4.4` and the `linux_arm64` line. This is
  the proof that ring and object_store build and link in manylinux_2_28; a failure here invokes the
  dependency-floor rule above.
- **Verify:** see frontmatter.
- **Touches:** `Cargo.toml`, `Cargo.lock`, `src/crawl.rs`, `src/s3.rs`, `src/bin/xdu.rs`, `tests/`,
  `doc/xdu.1.scd`, `README.md`, `AGENTS.md`, `.agents/factory/invariants.md`,
  `.github/workflows/test.yaml`.

## Phase P4 — xdu-view browses an s3:// index in place
**Satisfies:** R2, R6, R9 · **Depends on:** P3 · **Size:** ~0.5 day
**Goal:** `xdu-view -i s3://…` shows what it shows locally. This is the designed cut: if the cycle
runs long, P4 is dropped through a GOAL amendment (never silently), and P5 still runs.

- [ ] `src/view_query.rs` (new): the 4 distinct queries and `DirEntry` move out of
  `src/bin/xdu-view.rs`, taking `&IndexLocator` and going through `parquet_source`; the S3 arm also
  binds the data-derived `path LIKE` prefix (PLAN §2.9). Local SQL text unchanged.
- [ ] `src/bin/xdu-view.rs`: `App` holds `IndexLocator`; S3 prologue as `xdu-find`'s; terminal
  setup still after `App::new`; §12's restore guard untouched.
- [ ] Wire tests (`view_` prefix), all through the proxy with no whole-object GET: local vs S3
  `Vec<DirEntry>` equal for root, each partition, a nested directory, every `SortMode`, and a filter
  set; R4 refusals through `run_view` (no TUI) with empty `HOME`/`TMPDIR`/cwd left empty.
- [ ] `doc/xdu-view.1.scd`: as `xdu-find`'s page in P2; CI literal counts.
- **Run B evidence:** test legs as above with N ≥ 34; the Packaging job's literal step green with
  the new `xdu-view.1` checks.
- **Verify:** see frontmatter.
- **Touches:** `src/view_query.rs`, `src/lib.rs`, `src/bin/xdu-view.rs`, `tests/`,
  `doc/xdu-view.1.scd`, `.github/workflows/test.yaml`.

## Phase P5 — Close the cycle: full S3 suite, gate literals, deferral ledger
**Satisfies:** R9, R10 · **Depends on:** P4 · **Size:** ~0.25 day
**Goal:** the whole suite green with nothing ignored, the man-page gate asserting every new literal
by count, and every deferral recorded.

- [ ] Run the full suite through `verify/wire.sh "" 34`: every listed test passed.
- [ ] Re-derive every new literal count on all four pages from the rendered text; confirm the CI
  list and `AGENTS.md` agree (§13).
- [ ] **Deferral ledger.** Walk P1–P4's bodies and commits for "do not fix here", "known
  limitation", "follow-up", "deferred". Each must have an `issues/` file (from
  `templates/ISSUE.md`) and a `ROADMAP.md` entry. At minimum:
  `issues/s3-index-rm.md`, `issues/s3-credential-chain.md`,
  `issues/local-reader-bound-parameters.md` (GOAL Non-goals) and `issues/s3-proxy-support.md`
  (PLAN deviation 6). Update the `S3 as an index target` ROADMAP entry to point at the spec.
- [ ] If P4 was cut by amendment, record `issues/s3-index-view.md` and its ROADMAP entry here.
- **Run B evidence:** test legs as above with N ≥ 34 (the final `<min>` in the workflow equals the
  listed count); the Packaging job's literal step green on all four pages.
- **Verify:** see frontmatter.
- **Touches:** `issues/`, `ROADMAP.md`, `.github/workflows/test.yaml`, `AGENTS.md`, `doc/*.scd`.

---

## How `xdu-build` drives this

1. `next_phase.py` prints the next actionable phase (statuses are authoritative; the
   `current_phase` pointer is reconciled against them).
2. Pre-flight: clean tree, on `branch`, `base` reachable.
3. Execute every `[ ]` in the phase (consult `PLAN.md` / `research/` for detail).
4. Run the phase's `verify:` command — never advance on a checkbox alone.
5. Amend this file freely if reality diverges (regenerate frontmatter with `set_phase.py`; note the
   amendment in the commit body). STOP and escalate only on a **`GOAL.md` contradiction**.
6. Mark the phase `done`, advance `current_phase`, `--touch`; one `[feature]` commit; stop and
   report.
