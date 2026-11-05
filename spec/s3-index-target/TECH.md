---
slug: s3-index-target
title: Build an index into S3 and query it in place
kind: feature
appetite: big
status: in_progress
branch: feature/s3-index-target
base: main
current_phase: P1
last_updated: '2026-11-05'
phases:
- id: P1
  name: Loopback endpoint, resolver, S3 connection; xdu-rm refuses s3://
  status: in_progress
  satisfies:
  - R5
  - R7
  - R8
  - R10
  depends_on: []
  parallel: false
  hammerable: false
  hill: downhill
  verify: 'set -eu; sh tests/s3-wire.sh harness_ 6; cargo test --locked --lib s3::
    >target/p1-lib.log 2>&1; grep -qE "test result: ok. [1-9][0-9]* passed" target/p1-lib.log;
    for t in the_secret_statement_text_is_fixed_and_names_every_key_httpfs_would_fall_back_on
    the_secret_statement_addresses_by_endpoint the_writer_and_the_readers_address_the_same_host
    a_region_that_would_name_another_host_is_refused_for_aws a_comment_after_a_section_header_keeps_the_profiles_apart
    a_malformed_section_header_takes_no_keys_from_or_into_another_profile section_headers_follow_the_shared_file_rules;
    do grep -qF "test s3::tests::$t ... ok" target/p1-lib.log; done; if grep -nF ''sql.push_str(",
    '' src/s3.rs; then echo "an optional CREATE SECRET clause is back" >&2; exit 1;
    fi; cargo test --locked --test rm_tests --test offline_tests --test find_tests
    --test s3_tests; if grep -rnE "let _ = .*load_extension|load_extension\(.*\)\.ok\(\)"
    src; then exit 1; fi; grep -qF "include-ignored" .github/workflows/test.yaml;
    grep -qF "fetch.sh" .github/workflows/test.yaml; grep -qF ''sh tests/s3-wire.sh
    ""'' .github/workflows/test.yaml; if grep -rnE ''(^|[^a-z])spec/'' .github/workflows/;
    then echo "a workflow runs something under spec/" >&2; exit 1; fi; cargo clippy
    --locked --all-targets --all-features -- -D warnings; cargo fmt --all -- --check;
    echo P1-GATE-GREEN'
- id: P2
  name: xdu-find queries an s3:// index in place (demo path)
  status: done
  satisfies:
  - R2
  - R3
  - R4
  - R6
  - R9
  depends_on:
  - P1
  parallel: false
  hammerable: false
  hill: downhill
  attempts: 1
  verify: set -eu; sh spec/s3-index-target/verify/wire.sh find_ 14; cargo test --locked
    --lib; cargo test --locked --test find_tests --test offline_tests --test version_tests
    --test s3_tests; .agents/factory/bin/temp_index.sh sh -c "xdu-find -i \"\$XDU_INDEX\"
    --count"; grep -qF "XDU_EXTENSION_DIR" doc/xdu-find.1.scd; grep -qF "AWS_ENDPOINT_URL_S3"
    doc/xdu-find.1.scd; grep -A20 "check share/man/man1/xdu-find.1" .github/workflows/test.yaml
    | grep -qF "XDU_EXTENSION_DIR"; grep -qF "s3://" README.md; cargo clippy --locked
    --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; echo
    P2-GATE-GREEN
- id: P3
  name: xdu -o s3:// builds the index into the bucket
  status: in_progress
  satisfies:
  - R1
  - R4
  - R5
  - R9
  depends_on:
  - P2
  parallel: false
  hammerable: false
  hill: downhill
  attempts: 1
  verify: 'set -eu; sh tests/s3-wire.sh write_ 11; cargo test --locked; .agents/factory/bin/temp_index.sh
    sh -c "xdu-find -i \"\$XDU_INDEX\" --count"; grep -qF "AWS_PROFILE" doc/xdu.1.scd;
    grep -A20 "check share/man/man1/xdu.1" .github/workflows/test.yaml | grep -qF
    "AWS_PROFILE"; for c in openssl-sys native-tls aws-lc-sys; do if out=$(cargo tree
    --locked -e normal --target all -i $c 2>&1); then echo "$c is in the runtime tree"
    >&2; exit 1; fi; case "$out" in *"did not match any packages"*) ;; *) printf ''%s\n''
    "$out" >&2; echo "cargo tree failed while looking for $c" >&2; exit 1;; esac;
    done; cargo tree --locked -e normal --target all -i ring >/dev/null; if sed -n
    ''1,/^#\[cfg(test)\]/p'' src/s3.rs | grep -nE ''\{(err|e|error|source)(:[^}]*)?\}'';
    then echo "src/s3.rs formats a raw client error into a message" >&2; exit 1; fi;
    cargo test --locked --lib s3:: >target/p3-lib.log 2>&1; for t in error_detail_keeps_the_status_and_code_and_drops_the_body
    the_sinks_errors_strike_the_secret_and_token; do grep -qF "test s3::tests::$t
    ... ok" target/p3-lib.log; done; cargo clippy --locked --all-targets --all-features
    -- -D warnings; cargo fmt --all -- --check; echo P3-GATE-GREEN'
- id: P4
  name: xdu-view browses an s3:// index in place
  status: done
  satisfies:
  - R2
  - R6
  - R9
  depends_on:
  - P3
  parallel: false
  hammerable: true
  hill: downhill
  verify: set -eu; sh spec/s3-index-target/verify/wire.sh view_ 5; cargo test --locked;
    grep -qF "XDU_EXTENSION_DIR" doc/xdu-view.1.scd; grep -qF "AWS_ENDPOINT_URL_S3"
    doc/xdu-view.1.scd; grep -A20 "check share/man/man1/xdu-view.1" .github/workflows/test.yaml
    | grep -qF "XDU_EXTENSION_DIR"; cargo clippy --locked --all-targets --all-features
    -- -D warnings; cargo fmt --all -- --check; echo P4-GATE-GREEN
- id: P5
  name: 'Close the cycle: full S3 suite, gate literals, deferral ledger'
  status: in_progress
  satisfies:
  - R9
  - R10
  depends_on:
  - P4
  parallel: false
  hammerable: false
  hill: uphill
  verify: 'set -eu; for f in s3-index-rm s3-credential-chain local-reader-bound-parameters
    s3-proxy-support; do test -s issues/$f.md; grep -qF "## Problem" issues/$f.md;
    grep -qF "issues/$f.md" ROADMAP.md; done; listed=$(cargo test --locked --test
    s3_tests -- --list --include-ignored 2>/dev/null | grep -c '': test$''); grep
    -qF "sh tests/s3-wire.sh \"\" $listed" .github/workflows/test.yaml; sh tests/s3-wire.sh
    "" "$listed"; cargo test --locked; grep -qF "include-ignored" .github/workflows/test.yaml;
    for f in .agents/factory/invariants.md .agents/factory/review-rubric.md .agents/skills/xdu-review/SKILL.md
    .agents/factory/templates/REVIEW.md; do for p in src/s3.rs src/view_query.rs src/engine.rs;
    do grep -qF "$p" "$f" || { echo "$f does not name $p" >&2; exit 1; }; done; done;
    risk=$(sed -n ''/^## High-risk files/,/^## Testing/p'' AGENTS.md); for p in src/s3.rs
    src/view_query.rs src/engine.rs; do case "$risk" in *"$p"*) ;; *) echo "AGENTS.md
    High-risk does not name $p" >&2; exit 1;; esac; done; testing=$(sed -n ''/^##
    Testing/,/^## Packaging/p'' AGENTS.md); for w in tests/s3-wire.sh include-ignored
    XDU_TEST_EXTENSION_DIR fetch.sh; do case "$testing" in *"$w"*) ;; *) echo "AGENTS.md
    Testing does not name $w" >&2; exit 1;; esac; done; grep -qF ''read_completion_marker_s3''
    .agents/factory/invariants.md; grep -qF ''marker_version_error'' .agents/factory/invariants.md;
    grep -qF ''`xdu DIR -o/--outdir DIR|s3://'' AGENTS.md; grep -qF ''`xdu-find -i/--index
    DIR|s3://'' AGENTS.md; grep -qF ''`xdu-view -i/--index DIR|s3://'' AGENTS.md;
    if grep -nE "^[[:space:]]*[''.]" doc/*.scd; then exit 1; fi; cargo clippy --locked
    --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; echo
    P5-GATE-GREEN'
review:
  last_reviewed_commit: 435239ce522752a6f894ac30f0aa237354b9e3e4
  verdict: changes-requested
  blocked_reason: 'review cycle 1: R5 endpoint fallback, profile-header parse, writer
    error echo; gate-list drift'
  cycle: 1
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
  *Amended at review cycle 1:* the gate is now `tests/s3-wire.sh`, a standing CI gate;
  `verify/wire.sh` is a pointer that execs it, kept so the recorded gates run.
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
- **Environment note (P4, not a gate change).** On the build host, `uv run --with pyyaml python`
  hung fetching PyYAML for Python 3.14 into a fresh cache. On the foreman's ruling, the factory
  scripts (`next_phase.py`, `set_phase.py`) ran under that host's Python 3.14 with `PYTHONPATH`
  pointed read-only at a PyYAML 6.0.3 already unpacked in the shared uv cache: the same
  interpreter and library `uv run` would supply. Nothing was written to that cache, and no
  `verify:` command changed.

## Review cycle 1 — remediation triage

[`REVIEW.md`](REVIEW.md) (cycle 1, changes-requested at `435239c`) raised six CONFIRMED findings.
The reviewer recommended treating the human gate as applying (finding 2 widens the lists to
`src/s3.rs`, where findings 1, 3 and 4 sit). **The gate judge confirmed the gate applies and ruled:
fix all six now, with no stop.** Widening the five harness path lists rides in this diff on that
ruling, though `xdu-build` Step 2.3 keeps harness changes off product branches (META F8).

- **F1 (HIGH) → P1.** The reader's `CREATE SECRET` bound `ENDPOINT` only for an override, so with
  AWS resolved `httpfs` fell back to its `s3_endpoint` setting, which it fills from
  `DUCKDB_S3_ENDPOINT`. Fixed by making the statement text fixed: every key `httpfs` would take
  from an environment-filled setting is bound. The resolver names the host for both directions
  (`S3Settings::endpoint_authority`: the override, else `s3.<region>.amazonaws.com`); the writer
  now gives object_store that host explicitly (`client_endpoint_url`), the same URL it built
  implicitly. Class sweep (probed against the pinned `httpfs` v1.4.4): at load it fills
  `s3_access_key_id`, `s3_secret_access_key`, `s3_session_token`, `s3_region` (from `AWS_*`) and
  `s3_endpoint`, `s3_use_ssl`, `s3_requester_pays`, `s3_kms_key_id` (from `DUCKDB_S3_*`); it reads
  no proxy variable, no `URL_STYLE` and no `URL_COMPATIBILITY_MODE` from the environment. Three
  more fallbacks were live: `SESSION_TOKEN` (bound only when present), `REQUESTER_PAYS` and
  `KMS_KEY_ID` (never bound). All are bound now: an empty token (no header is sent, proved on the
  wire), requester-pays off as the writer's is, an empty KMS key (it marks writes only). Sibling:
  with no override the region becomes part of the host, so the resolver refuses a region that is
  not `[a-z0-9-]` (naming its source, not its value); the three reader/writer man pages say so.
- **F3 (MEDIUM) → P1.** `Ini::parse` took a header only when the line ended in `]`. Now any line
  opening with `[` ends the section above; `[name]` may carry a trailing `#`/`;` comment, and any
  other malformed header starts a section no lookup can name, so its keys reach no profile
  (sibling: `[other] trailing` and an unclosed `[other` merged the same way). The sweep of the
  shared-file rules found the rest already right and now pinned by unit tests: whitespace inside
  the brackets, case-sensitive names and `profile` prefix, `[profile x]` ignored in the
  credentials file, `[profile default]` ahead of `[default]`. Not changed, recorded for the
  foreman: a `#`/`;` after a property value is part of the value here, as botocore reads it; the
  AWS SDKs' profile parser strips it when whitespace precedes it. Either way a wrong value fails
  closed (a refused signature or a named error).
- **F4 (MEDIUM) → P3.** `S3Sink::request_error` printed object_store's text, which quotes the
  response body. It now keeps the client's words up to the body, then the status and the body's
  S3 `<Code>`, and strikes the secret and token from the text as a last guard. Class sweep: every
  `S3Sink` request routes through `request_error`; `connect`'s and `load_ini`'s contexts carry no
  body, header or credential; nothing presigns; object_store's retry `info!` lines go nowhere,
  since no binary installs a logger. The reader (`lib::s3_request_error`) already dropped the text
  on 401/403; the new wire test drives it against a 400 `InvalidToken` body too, and DuckDB's text
  carries no body.
- **F2 (HIGH, harness) → P5.** `src/s3.rs`, `src/view_query.rs` and `src/engine.rs` join the path
  list in `invariants.md`, `review-rubric.md`, `xdu-review`'s SKILL.md, `templates/REVIEW.md` and
  `AGENTS.md` High-risk (which already named `src/engine.rs`). Widening only.
- **F5 (MEDIUM) → P1 (workflow) and P5 (manual).** The wire gate moved to `tests/s3-wire.sh` and
  the workflow runs it from there with the minimum at the listed 43. The spec copy stays as a
  pointer that execs it, so the recorded phase gates still run. `AGENTS.md` Testing, Commands, the
  pre-release-gate bullet and the repository map document the tier. `invariants.md` §13's "record,
  not a gate" sentence stays true. The script no longer exports `XDU_TEST_S3_REQUIRED`, which
  nothing read.
- **F6 (MEDIUM) → P5.** §2b names `read_completion_marker_s3`, `marker_version_error` and
  `marker_completion_warning`, and the S3 bound (LIST, HEAD for the size, `MarkerRead::Oversized`
  refused unfetched). §5 describes the fixed statement. `AGENTS.md` CLI surface shows `s3://` for
  `-o` and `-i`, and `xdu-rm`'s refusal.
- **One commit for P1, P3 and P5.** They share same-commit obligations: the workflow's minimum must
  equal the listed count the new P1 and P3 tests raise; the resolver's region refusal pairs with
  its man-page sentence; and the gate's move changes the path every gate runs.
- **Run A / run B.** The fix changes `src/` and `.github/workflows/`, so the CI evidence in
  `evidence/p1-ci.md`, `p3-ci.md` and `p5-ci.md` no longer describes the head. This remediation is
  run A: local gates green, P1, P3 and P5 left `in_progress`. Run B reads the CI run of the pushed
  head (bullets below) and marks them done. The release workflow does not run on this diff (its
  pull-request paths name `src/engine.rs`, not `src/s3.rs`, and nothing in `Cargo.*` changed), so
  P1's and P3's release-leg evidence stays at `b6673d3`.

---

## Phase P1 — Loopback endpoint, resolver, S3 connection; xdu-rm refuses s3://
**Satisfies:** R5, R7, R8, R10 · **Depends on:** — · **Size:** ~0.5 day
**Goal:** the foundation every later phase stands on, proven end to end: a test can start an
SigV4-checking S3 endpoint on loopback, resolve credentials the way R5 says, open a DuckDB
connection that loads the pinned httpfs and holds those credentials only as bound parameters, and
read an object through the counting proxy. CI runs it on both legs. `xdu-rm` refuses `s3://`.

- [x] `src/s3.rs` (new, re-exported via `lib`): `IndexLocator`/`S3Locator` parse on `OsStr`
  (PLAN §2.1) with unit tests: `s3://b/p`, trailing `/`, empty bucket, userinfo, glob
  metacharacters, uppercase `S3://` stays local, non-UTF-8 stays local.
- [x] `src/s3.rs`: `S3Settings::resolve` over an `EnvSource` trait (PLAN §2.2, digest D4),
  hand-rolled INI, redacted `Debug`. Unit tests from fixture files: env beats profile; credentials
  file beats config; `AWS_PROFILE` selects `[profile x]`/`[x]`; `AWS_REGION` > `AWS_DEFAULT_REGION`
  > profile > `us-east-1`; `AWS_ENDPOINT_URL_S3` > `AWS_ENDPOINT_URL` > `services` > `endpoint_url`;
  path style iff an endpoint is set; half-set env credentials, no credentials, endpoint with a path
  → named errors that contain no secret.
- [x] `src/engine.rs`: `open_s3_connection(&S3Settings)` = `open_connection` + `load_extension
  ("httpfs")` + one prepared `CREATE SECRET … ?` (PLAN §2.3). Amend the absent-extension text to
  name the path and both remedies (copy `lib/xdu/extensions/` from a release tarball; set
  `XDU_EXTENSION_DIR`), keeping `assert_names_absent_file`'s substrings. `open_connection` stays
  byte-identical.
- [x] `Cargo.toml` dev-dependencies: `s3s = "=0.17.0"`, `s3s-fs = "=0.17.0"`, `hyper-util`, `tokio`
  (digest D6). Confirm the toolchain pin builds them.
- [x] `tests/common/s3.rs`: `TestS3` (in-process s3s-fs on `127.0.0.1:0`, `SimpleAuth`, temp root,
  `seed_index(local_index, prefix)` by copying into the root), the std-only counting proxy (passes
  `Host` through; forces `Connection: close` on both legs, or parses every request on a kept-alive
  connection, and asserts request count = connection count in the first case; records per request
  method, path, `Range`, response status, `Content-Range` and bytes to client) and the hermetic
  child `Command` builder. A refused bind or a missing extension panics, always.
- [x] `tests/s3_tests.rs` `harness_` tests: an `open_s3_connection` reads a seeded object through
  the proxy and the proxy saw it; a wrong secret is refused with a 403-class error that does not
  contain the secret; a stray `DUCKDB_S3_ENDPOINT` in the environment does not redirect the
  connection (the bound secret wins, digest D2). Per amended R5/R6, on the connection
  `open_s3_connection` returns: `SELECT secret_string FROM duckdb_secrets()` contains
  `secret=redacted` and not the secret; no row of `duckdb_settings()` contains the secret (or the
  session token); and every SQL string `open_s3_connection` builds is exposed to a unit test
  (a pure `fn` returning the statement text) that asserts neither secret value occurs in it.
- [x] Always-run tests: `xdu-find -i s3://…` with `XDU_EXTENSION_DIR` set to an empty directory
  fails naming that path and both remedies — assert the phrase "release tarball" and
  `XDU_EXTENSION_DIR`, not `lib/xdu/extensions` (the default path already contains it). With a
  garbage `httpfs.duckdb_extension` in `XDU_EXTENSION_DIR`, local `xdu-find`, `xdu-rm --dry-run`
  and `xdu -o <local dir>` answer correctly **with empty stderr** (a swallowed load error would
  print or would have to be discarded, which the verify grep refuses). `offline_tests.rs` untouched
  and green. *Amended in run A:* `xdu` logs `INFO` records to stderr by design, so its stderr
  cannot be empty; the test asserts it is non-empty, every line is an `INFO` record, and none
  names `httpfs` or an extension. `xdu-find` and `xdu-rm` keep the empty-stderr assertion.
  (The xdu-find S3 branch that makes the first test reachable is the minimal prologue of P2's
  §2.6 — parse, resolve, `open_s3_connection`, then `bail!` "not yet" is acceptable here only if
  P2 replaces it in the same cycle; prefer landing the real prologue.)
  *Run A landed the minimal prologue:* parse, resolve, `open_s3_connection`, then
  `bail!("…: querying an S3 index in place is not yet supported")`. The real prologue needs
  §2.5's marker read, which is P2's; P2 replaces the `bail!`.
- [x] `src/bin/xdu-rm.rs`: refuse an `s3://` locator right after parse with "S3 indices are not yet
  supported by xdu-rm", exit non-zero (PLAN §2.7); `tests/rm_tests.rs` covers `-i` and `XDU_INDEX`
  and asserts nothing else ran (no extension load, no pool). `doc/xdu-rm.1.scd`: one sentence
  under `-i`; re-count `XDU_INDEX` if the sentence names it.
- [x] `.github/workflows/test.yaml`, both legs: build `extension-probe`, `fetch.sh` into
  `$RUNNER_TEMP/xdu-ext`, then an "S3 tests" step after Test:
  `XDU_TEST_EXTENSION_DIR=… sh spec/s3-index-target/verify/wire.sh "" <min>`, the one guard script
  both verify and CI run (cargo's exit status read directly; passed count equal to the
  `--list --include-ignored` count). Raise `<min>` in the same commit each later phase adds tests.
  The step's last line, `s3-wire: '' N of N passed`, is what each run B reads. *Run A set
  `<min>` to 7, the whole file's listed count (5 `harness_` wire tests and 2 always-run tests),
  rather than 5: every test in the file runs under `""`, so the floor equals the list.*
- **Run B evidence:** in the test workflow, jobs `Test (ubuntu-24.04)` and `Test (macos-15)`,
  step "S3 tests": `s3-wire: '' N of N passed` with N ≥ 5 (the ubuntu line is the first observed
  httpfs load by a debug build on linux_amd64). In the release workflow (it runs because this phase
  touches `Cargo.toml` and `src/engine.rs`), jobs `Build (x86_64-unknown-linux-gnu)` and
  `Build (aarch64-unknown-linux-gnu)`: `extension-check: ok platform=linux_amd64 engine=v1.4.4`
  and `extension-check: ok platform=linux_arm64 engine=v1.4.4`. *Recorded by run B in
  [`evidence/p1-ci.md`](evidence/p1-ci.md) (tests run 42003486290, release run 42003486169).*
- **Verify:** see frontmatter (lib `s3::` unit tests, always-run suites, `harness_` wire tests with
  `verify/wire.sh`, no discarded `load_extension` result, the CI step present, clippy, fmt).
- **Touches:** `src/s3.rs`, `src/lib.rs`, `src/engine.rs`, `src/bin/xdu-find.rs` (prologue only),
  `src/bin/xdu-rm.rs`, `Cargo.toml`, `Cargo.lock`, `tests/common/`, `tests/s3_tests.rs`,
  `tests/rm_tests.rs`, `doc/xdu-rm.1.scd`, `.github/workflows/test.yaml`.

> **Amendment (P1, review cycle 1 remediation):** F1, F3 and the workflow half of F5 (triage
> above). Gate retuned: `tests/s3-wire.sh harness_ 6`, the new resolver and statement unit tests
> by name, and two class predicates: no optional `CREATE SECRET` clause (`sql.push_str(", `
> absent from `src/s3.rs`) and no workflow running anything under `spec/`.
>
> - [x] Red first, F1: `harness_stray_duckdb_s3_endpoint_is_not_taken_when_the_resolver_picks_aws`
>   on the old code: "1 connection(s) reached the DUCKDB_S3_ENDPOINT listener; first bytes
>   [[22, 3, 1, …]]; recorder saw ["CONNECT 127.0.0.1:<port>"]". Fixed: 0 connections, and every
>   request the reader made was `CONNECT 127.0.0.s3.us-east-1.amazonaws.com:443` to a loopback
>   recorder that tunnels nowhere else. The child sets `http_proxy` on its own connection so
>   nothing leaves loopback.
> - [x] Red first, the sweep: the stray-environment test now also sets `AWS_SESSION_TOKEN`,
>   `AWS_REGION`, `DUCKDB_S3_USE_SSL`, `DUCKDB_S3_REQUESTER_PAYS` and `DUCKDB_S3_KMS_KEY_ID`,
>   asserts `httpfs` took each, and that no request carried `x-amz-security-token`,
>   `x-amz-request-payer` or an SSE header. Old code: "a request carried x-amz-request-payer from
>   the stray environment" (the same request carried `x-amz-security-token`). Fixed: green.
> - [x] Red first, F3: the new unit tests failed on the old parser (`default` resolved to
>   `other-id`), and `find_credentials_from_a_profile_in_the_shared_files` now drives both header
>   forms through `xdu-find`; against the old parser it fails with "no S3 credentials".
> - [ ] Run B: evidence below.
>
> **Run B evidence (cycle 1):** jobs `Test (ubuntu-24.04)` and `Test (macos-15)`, step "S3 tests":
> `s3-wire: '' 43 of 43 passed`. Record in `evidence/p1-ci.md` under a "Review cycle 1" heading.

## Phase P2 — xdu-find queries an s3:// index in place (demo path)
**Satisfies:** R2, R3, R4, R6, R9 · **Depends on:** P1 · **Size:** ~0.75 day
**Goal:** `xdu-find -i s3://bucket/prefix` (or `XDU_INDEX`) returns what it returns locally, pays
for footers and selected row groups only, refuses bad markers and bad endpoints with a named cause,
and binds everything S3 sends to SQL. The demo works after this phase.

- [x] `src/lib.rs`: `index_glob_s3` beside `index_glob`; `parquet_source(&IndexLocator, partition,
  filename) -> (String, Vec<String>)` with a unit test pinning the local strings byte for byte
  (PLAN §2.4).
  *Run A:* both globs share one private tail, `index_glob_tail`, so `index_glob`'s output is
  unchanged.
- [x] `src/lib.rs`: `read_completion_marker_s3` (glob, then size, then content; cap before any
  GET), `classify_s3_error`, and gates taking a pre-read `MarkerRead` (PLAN §2.5). Local marker
  path keeps its FIFO-safe read.
  *Run A:* the gates are `marker_version_error` / `marker_completion_warning` over a pre-read
  `MarkerRead`; `index_version_error(&Path)` and `index_completion_warning(&Path)` stay as the
  local wrappers, so `xdu-view` and `xdu-rm` are untouched. `MarkerRead` gains `Oversized(size)`,
  which only the S3 read returns; its refusal names the size and the cap. The glob step lists
  `<prefix>/**` with `LIMIT 1`: `*` matches the marker but not a prefix holding chunks only. A
  404 on that listing is "bucket does not exist", an empty listing is "the prefix holds no
  objects", and a 404 on the marker's `HEAD` is `Absent` (the version gate's "carries no index
  format version", as locally).
- [x] `src/bin/xdu-find.rs`: full S3 prologue (PLAN §2.6); all 7 statements through
  `parquet_source` with `params_from_iter`. Local behaviour unchanged.
- [x] Wire tests (`find_` prefix):
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

  *Amended in run A:*
  - **Endpoint fidelity for the quote test.** s3s-fs echoes `EncodingType=url` on ListObjectsV2
    but sends keys raw, so its XML carries `'` as `&apos;`; httpfs always asks for url encoding
    and decodes percent escapes, not XML entities, so a quoted prefix listed as no files. S3
    percent-encodes listed keys when asked. `tests/common/s3.rs` now wraps s3s-fs in
    `UrlEncodedListing`, which encodes `Key`, `Prefix`, `StartAfter`, `Delimiter` and common
    prefixes when `encoding-type=url` is asked and passes every other operation the tests use
    straight through (dev-dependency `async-trait`, already in the lockfile through s3s). P3's
    writer operations (PUT, DELETE, multi-DELETE) are among those passed through.
  - **15 `find_` tests, 22 in the file**; the CI step's minimum rises from 7 to 22. Parity runs
    over `varied_source_tree` (1400 files in `alice` nested, `bob` and `__root__`, every size,
    atime and mtime distinct and half a step off a day boundary, long hex names so every chunk
    exceeds the footer read). The cost fixture is `encoded_index`, 8 chunks of 9500 rows over
    `__root__`, `alice`, `bob`, `carol`, through `crawl::encode_chunk`, about 16 MiB.
  - **Printed ratios.** The cost test and its negative control write `s3-count-bytes:` and
    `s3-scan-bytes:` lines to the process's stderr directly, past libtest's capture, so
    `wire.sh`'s log of a passing run, and CI's, keeps them.
  - **An engine assertion avoided, not fixed.** The bundled DuckDB's debug build aborts
    (`Assertion failed: (min_val <= input)`, `compress_integral.cpp`) on the **local** query
    `-u alice --min-size 500 -f size` over this fixture; the release build answers it correctly.
    The local SQL is byte-identical to before this phase, so the filter test uses other size and
    sort combinations. Reported to the foreman.
- [x] Probe while building (record the result in the commit body): `read_parquet(?, filename=true)`
  and a bound glob inside a CTE both bind (`01` §2 left them unprobed).
  *Run A:* both bind and count all rows against s3s-fs (commit body has the detail).
- [x] `doc/xdu-find.1.scd`: `s3://BUCKET/PREFIX` under `-i`; a paragraph on S3 indices
  (ranged reads, nothing copied locally, proxy variables not used, `xdu-rm` refuses); `ENVIRONMENT`
  gains the ten `AWS_*` names and `XDU_EXTENSION_DIR` with the `cargo install` remedy. Add the
  page's literals to the CI check with counts derived from the rendered page (`05` §3 hazards:
  `AWS_ENDPOINT_URL` counts include `AWS_ENDPOINT_URL_S3`; an example naming `XDU_INDEX` bumps its
  count). `AGENTS.md:277`'s sentence names `XDU_EXTENSION_DIR`.
  *Run A:* scdoc is not installed in the build environment, so the page was not rendered here.
  The counts were derived from the source with markup removed and whitespace stripped, the way
  CI normalizes the render; the Packaging job is the proof (run B evidence).
- [x] `README.md`: `### Indexes in S3` (query half) before `### Querying with DuckDB`; one clause at
  `:99-101` that S3 indices need `httpfs`.
- [x] `AGENTS.md` and `invariants.md` §5: S3 locators and partition names are bound; local raw
  `format!` note stands; `open_s3_connection`'s `CREATE SECRET` joins the engine surface.
- **Run B evidence:** jobs `Test (ubuntu-24.04)` and `Test (macos-15)`, step "S3 tests":
  `s3-wire: '' N of N passed` with N ≥ 19; job `Packaging (man + completions generate)`, step
  "Assert critical literals survive into the published man-page text", green with the new
  `xdu-find.1` checks; the R3 test's printed ratio line from the ubuntu leg. *Recorded by run B
  in [`evidence/p2-ci.md`](evidence/p2-ci.md) (tests run 42012803115, N = 22 on both legs,
  ubuntu ratio 0.824%; release run 42012803137).*
- **Verify:** see frontmatter.
- **Touches:** `src/lib.rs`, `src/s3.rs`, `src/bin/xdu-find.rs`, `tests/`, `doc/xdu-find.1.scd`,
  `README.md`, `AGENTS.md`, `.agents/factory/invariants.md`, `.github/workflows/test.yaml`.

## Phase P3 — xdu -o s3:// builds the index into the bucket
**Satisfies:** R1, R4, R5, R9 · **Depends on:** P2 · **Size:** ~1 day
**Goal:** `xdu <src> -o s3://bucket/prefix` writes the index a local run writes, chunk by chunk,
with §2b's ordering, and a re-run leaves no stale chunk. Readers from P2 answer from it.

- [x] `Cargo.toml`: `object_store = { version = "0.14.2", default-features = false, features =
  ["aws-base", "reqwest", "ring"] }`, `reqwest = { version = "0.13", default-features = false,
  features = ["rustls-no-provider"] }`, `rustls = { version = "0.23", default-features = false,
  features = ["ring", "std"] }`, `tokio` (`rt`, `rt-multi-thread`), `futures`, `bytes` (digest D3).
  Install the ring provider once before any client.
  *Run A:* 30 crates new to `Cargo.lock` (the digest counted 26 against an older lock); the
  provider is installed in `S3Sink::connect`, before the client is built.
- [x] `src/crawl.rs`: `IndexSink`, `LocalSink` (byte-for-byte today's files, rename and first-gap
  prune), `PartitionBuffer` encodes into a `Vec<u8>` passed by value (PLAN §2.8, port from
  `32ba641` with digest D9's changes). Existing `crawl_tests` stay green unchanged.
  *Run A:* the trait is `preflight` (default no-op), `check_top_level` (default accepts),
  `clear_marker`, `put_chunk`, `finalize_partition`, `write_marker` and `marker_location`, so
  `xdu.rs` drives both sinks through one sequence. `tests/crawl_tests.rs` is untouched and green;
  the three `PartitionBuffer` unit tests in `crawl.rs` build a `LocalSink` instead of passing a
  path.
- [x] `src/s3.rs`: `S3Sink` (owned runtime; `AmazonS3Builder::new()` from resolver output only;
  `no_proxy()` reqwest connector; bounded retry and timeouts; PUT at the final key; LIST + DELETE
  of every higher id; marker DELETE/PUT; key round-trip check for partition names).
  *Amended in run A:*
  - **Single DELETEs.** object_store's `delete` on S3 is a bulk `POST ?delete` by default; the
    sink sets `with_disable_bulk_delete(true)`, so every removal is one DELETE the proxy (and an
    S3 access log) names by key.
  - **A listing's status is in its text.** A failed LIST reaches the caller as a generic store
    error whose status sits only in the wrapped error's text (`status code: 403`), so the cause
    is read from there, as the readers read httpfs's; 401/403, 404 and connect/timeout errors
    get the readers' distinct wording ("refused the credentials", "does not exist", "cannot reach
    the S3 endpoint").
  - **Non-ASCII names are refused.** The round-trip check is the one PLAN names (what the client
    would percent-encode), and object_store encodes every non-ASCII byte, so a directory such as
    `café` is refused for an S3 output, as are `.`/`..`, controls, glob metacharacters and the
    characters AWS advises against. Building keys raw instead would admit non-ASCII names, at the
    price of an unprobed reader path; left to the foreman. *Foreman ruling at P3 run B:* keep
    refusing them; the raw-key alternative is a P5 ledger seed,
    `issues/s3-non-ascii-partition-names.md`.
  - **The prefix is checked too.** A prefix with an empty, `.` or `..` segment or a character the
    client encodes is refused before any request, since the writer's keys would differ from the
    readers' glob.
- [x] `src/bin/xdu.rs`: `-o` stays `PathBuf`; classify on `OsStr`; the S3 sequence of PLAN §2.8 with
  the marker DELETE after every rejecting check; local sequence untouched. Per amended R1, refuse
  a top-level name that is not UTF-8 or not safe as an S3 key segment and a DuckDB glob before the
  clear, exiting non-zero and naming the directory; per amended R4, an absent or empty prefix is a
  new index, while a missing bucket, an unreachable endpoint or rejected credentials fail.
  *Run A:* order is source checks, settings, `preflight` (the LIST), then in `crawl` the name
  check over **every** top-level directory (a `--partition` filter does not exempt one, as with
  reserved names), `build_work_queue`, and the marker DELETE. `-o` help names the `s3://` form.
- [x] Wire tests (`write_` prefix):
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

  *Run A:* 14 `write_` tests, 36 in the file; the CI step's minimum rises from 22 to 36. The
  read-only identity is a second key pair the test endpoint accepts and refuses every write for;
  a second injected failure refuses every PUT under one partition, so a run fails mid-crawl with
  earlier partitions' chunks already sent and later ones never started. The ordering test also
  counts the per-partition prune LISTs between chunk PUTs. Tests that read a written index back
  use one chunk a partition, so no ranged read covers a whole small chunk. The non-UTF-8 name
  case runs only where the filesystem allows such a name (the Linux leg; APFS refuses it) and
  prints a `write-unsafe-names:` line saying whether it ran.
- [x] Release-build check: the runtime dependency tree holds no `openssl-sys`, `native-tls` or
  `aws-lc-sys` (`cargo tree -e normal -i …`, in `verify:`), and does hold `ring`. Record in the
  commit body that the manylinux_2_28 leg's glibc/NEEDED check must pass on the first push (ring is
  proven there only as a build dependency).
  *Gate amendment at P3 run B, on the foreman's ruling (META F5):* the probe read cargo's exit
  status, so any cargo failure (an unreachable registry, a bad target) passed as "absent", and it
  looked at the host target only. The stored `verify:` now counts a crate absent only when
  `cargo tree` says `did not match any packages`, fails on any other non-zero exit, and asks
  with `--target all`, the `ring` probe included. Changed through `set_phase.py --verify-file`
  and read back byte for byte. Red-first: a bad target, a cold `CARGO_HOME` with no network,
  and a present crate (`ring` put in the forbidden list) each fail it; the real tree passes.
- [x] `doc/xdu.1.scd`: `-o` accepts `s3://BUCKET/PREFIX`; S3 paragraph (chunks sent as they
  finalize, nothing assembled locally, proxy variables not used); `ENVIRONMENT` gains the ten
  `AWS_*` names; CI literal counts. `README.md`: the build half of `### Indexes in S3`.
  *Run A:* the paragraph is a section, `S3 OUTPUT`. scdoc is not installed here, so the page was
  not rendered; counts were derived from the source with markup and whitespace stripped
  (`2x:s3://BUCKET/PREFIX`, `2x:AWS_PROFILE` with the example, `2x:AWS_ENDPOINT_URL`, the other
  seven names `1x`). The Packaging job is the proof.
- [x] `invariants.md` §2/§7 and the matching `AGENTS.md` text: S3 finalize = one PUT at the final
  key plus LIST prune; the runtime's threads in the budget.
  *Run A:* §2b gains the S3 pre-flight (LIST and name refusal) before the marker DELETE; the
  `AGENTS.md` source map gains `s3.rs`.
- **Run B evidence:** test legs as above with N ≥ 29. Release workflow, both `Build (…-linux-gnu)`
  jobs: step "Assert glibc floor" prints `… needs max GLIBC_2.x` lines with x ≤ 28 and no
  `floor breach`, `runtime breach` or `unexpected dynamic needs` line; then
  `extension-check: ok platform=linux_amd64 engine=v1.4.4` and the `linux_arm64` line. This is
  the proof that ring and object_store build and link in manylinux_2_28; a failure here invokes the
  dependency-floor rule above. *Recorded by run B in [`evidence/p3-ci.md`](evidence/p3-ci.md)
  (tests run 42020057385, N = 36 on both legs, the non-UTF-8 name case ran on ubuntu; release
  run 42020057315, every Linux binary at `GLIBC_2.28`, no breach line; the macOS x86_64 leg's
  cold-cache release build took 39m 29s inside its 90-minute timeout).*
- **Verify:** see frontmatter.
- **Touches:** `Cargo.toml`, `Cargo.lock`, `src/crawl.rs`, `src/s3.rs`, `src/bin/xdu.rs`, `tests/`,
  `doc/xdu.1.scd`, `README.md`, `AGENTS.md`, `.agents/factory/invariants.md`,
  `.github/workflows/test.yaml`.

> **Amendment (P3, review cycle 1 remediation):** F4 (triage above). Gate retuned: the wire gate
> at its new path with `write_ 11`, the `error_detail` and redaction unit tests by name, and the
> class predicate that no line of `src/s3.rs` outside its tests formats a raw client error
> (`{err}`, `{e}`, `{error}` or `{source}`); the old code has three.
>
> - [x] Red first: `write_refusal_bodies_that_echo_the_session_token_are_not_printed` on the old
>   code: "stderr carries a secret: … refused the credentials … (Generic S3 error: … Server
>   returned non-2xx status code: 403 Forbidden: <?xml …><Code>SignatureDoesNotMatch</Code>…
>   <Token-0>xdu-test-session-token-…</Token-0>". Fixed: the writer prints
>   "… - HTTP 403 SignatureDoesNotMatch" and "… - HTTP 400 InvalidToken"; `xdu-find` prints its
>   cause and no body in both cases.
> - [ ] Run B: evidence below.
>
> **Run B evidence (cycle 1):** the test legs' `s3-wire: '' 43 of 43 passed` (shared with P1).
> No release run: the release workflow's paths do not cover this diff, so the release-leg
> evidence in `evidence/p3-ci.md` stays at `b6673d3`. Record in `evidence/p3-ci.md`.

## Phase P4 — xdu-view browses an s3:// index in place
**Satisfies:** R2, R6, R9 · **Depends on:** P3 · **Size:** ~0.5 day
**Goal:** `xdu-view -i s3://…` shows what it shows locally. This is the designed cut: if the cycle
runs long, P4 is dropped through a GOAL amendment (never silently), and P5 still runs.

- [x] `src/view_query.rs` (new): the 4 distinct queries and `DirEntry` move out of
  `src/bin/xdu-view.rs`, taking `&IndexLocator` and going through `parquet_source`; the S3 arm also
  binds the data-derived `path LIKE` prefix (PLAN §2.9). Local SQL text unchanged.
  *Amended in run A:*
  - **Five statement texts, not four.** `load_directory` (list view) and
    `make_directory_column_tree` (tree view) send the same statement in two whitespace layouts.
    Merging them would change one view's local SQL text, which R6 keeps as it is, so each keeps
    its own builder (`list_directory_sql`, `tree_directory_sql`) over one shared set of parts
    (`DirectoryQuery`) and one row reader. `load_partitions` and `make_partition_column` were
    byte-identical and share `partitions_sql`.
  - **Pure builders.** Each statement is built by a `*_sql` function returning `(text, bound
    values)`; `root_files`, `partitions`, `partition_root`, `list_directory` and
    `tree_directory` run them. The in-memory `sort_entries` (§9), the `__root__` merge, the `..`
    entry and the status lines stay in the binary.
  - **The byte-for-byte proof.** The unit test
    `local_view_sql_is_byte_for_byte_what_xdu_view_sent` compares every local statement, over 3
    filter sets, all 7 sorts and 3 directories (quotes included), with a frozen copy of the
    binary's old builders kept in the test module. The raw SQL blocks of both the new builders
    and the frozen copy were cut by script from the binary at `caf72b4`, so no whitespace was
    retyped; the hand-written fragments were checked against that file by grep.
- [x] `src/bin/xdu-view.rs`: `App` holds `IndexLocator`; S3 prologue as `xdu-find`'s; terminal
  setup still after `App::new`; §12's restore guard untouched.
  *Run A:* the local arm is the old code moved into the match, and still opens its connection
  after the filters are parsed; the S3 arm's connection is the one the marker was read on.
  `cli.rs` is unchanged.
- [x] Wire tests (`view_` prefix), all through the proxy with no whole-object GET: local vs S3
  `Vec<DirEntry>` equal for root, each partition, a nested directory, every `SortMode`, and a filter
  set; R4 refusals through `run_view` (no TUI) with empty `HOME`/`TMPDIR`/cwd left empty.
  *Run A:* 5 `view_` tests, 41 in the file; the CI step's minimum rises from 36 to 41.
  - The parity tests run the `view_query` functions on an in-test connection to each index,
    since the TUI never runs in CI. Every directory of `alice` (above its root, the root, a
    nested directory and the one below it) and `bob`'s root, in both layouts, under all 7
    sorts, and the partition list and `__root__` files; a filtered test runs 7 filter sets
    under 3 sorts. The statements order by nothing else, so rows that tie on the sort key (every
    file has a count of 1) may come in either order: the comparison holds the sequence of sort
    keys equal and the entries equal as a set, which for a unique key is exact equality.
  - The quote test puts a `'` in the prefix and in a listed directory (`alice/b/it's`). The
    quoted directory is listed over S3 only: the local statement writes the prefix into a
    `LIKE` literal, which the quote ends (invariant §5's local note).
  - The R4 test drives the `xdu-view` binary, whose refusals all come before the terminal is
    touched: the seven causes of `xdu-find`'s matrix, `XDU_INDEX`, and an empty
    `XDU_EXTENSION_DIR`, each with empty stdout and its own cause. The accepted path of the
    binary is not driven: past `App::new` it needs a terminal.
- [x] `doc/xdu-view.1.scd`: as `xdu-find`'s page in P2; CI literal counts.
  *Run A:* scdoc is not installed here, so the page was not rendered. The counts were derived
  from the source with markup and whitespace stripped, by a script that reproduces CI's
  `xdu-find.1` list exactly when run on that page; `xdu-view.1` carries the same list. The page
  also says the tree view's file preview reads the file itself, not the index. `README.md`'s
  `### Indexes in S3` names `xdu-view` beside `xdu-find`; `AGENTS.md` (source map, the
  duplicated-literal sentence) and `invariants.md` §5 name `view_query`.
- **Run B evidence:** test legs as above with N ≥ 34; the Packaging job's literal step green with
  the new `xdu-view.1` checks. *Recorded by run B in [`evidence/p4-ci.md`](evidence/p4-ci.md)
  (tests run 42034745391, N = 41 on both legs, all 5 `view_` tests named; it tested the merge
  ref `b005caf` onto `main` at `a0cdd95`, which differs from `ebe5a48` only in PR #32's
  `ROADMAP.md` and `issues/` edits; release run 42034745344 green, every Linux binary at
  `GLIBC_2.28`).*
- **Verify:** see frontmatter.
- **Touches:** `src/view_query.rs`, `src/lib.rs`, `src/bin/xdu-view.rs`, `tests/`,
  `doc/xdu-view.1.scd`, `.github/workflows/test.yaml`, `README.md`, `AGENTS.md`,
  `.agents/factory/invariants.md`.

## Phase P5 — Close the cycle: full S3 suite, gate literals, deferral ledger
**Satisfies:** R9, R10 · **Depends on:** P4 · **Size:** ~0.25 day
**Goal:** the whole suite green with nothing ignored, the man-page gate asserting every new literal
by count, and every deferral recorded.

- [x] Run the full suite through `verify/wire.sh "" 34`: every listed test passed.
  *Run A:* 41 of 41 locally; the workflow's minimum is 41, the listed count.
- [x] Re-derive every new literal count on all four pages from the rendered text; confirm the CI
  list and `AGENTS.md` agree (§13).
  *Run A:* scdoc is not installed here, so no page was rendered. Every `check` spec in the
  workflow (51 across the four pages) was re-derived from the `.scd` sources with markup and
  whitespace stripped, as CI normalizes the render: every count and presence matches. `AGENTS.md`
  names the same counted classes (`XDU_INDEX`, `XDU_JOBS`, `XDU_EXTENSION_DIR`, the `AWS_*`
  names, `xdu.1`'s two `s3://BUCKET/PREFIX`). The Packaging job is the proof.
- [x] **Deferral ledger.** Walk P1–P4's bodies and commits for "do not fix here", "known
  limitation", "follow-up", "deferred". Each must have an `issues/` file (from
  `templates/ISSUE.md`) and a `ROADMAP.md` entry. At minimum:
  `issues/s3-index-rm.md`, `issues/s3-credential-chain.md`,
  `issues/local-reader-bound-parameters.md` (GOAL Non-goals) and `issues/s3-proxy-support.md`
  (PLAN deviation 6). Update the `S3 as an index target` ROADMAP entry to point at the spec.
  Also a seed for the bundled DuckDB's debug-build assertion found in P2 (foreman ruling at P2
  run B): `xdu-find -u alice --min-size 500 -f size` on a local index aborts a debug build with
  `Assertion failed: (min_val <= input)` in `compress_integral.cpp`, while the release build
  answers correctly. The P2 filter test avoids that combination (see P2's run A amendments). File
  it as an `issues/` file with a `ROADMAP.md` entry.
  Also `issues/s3-non-ascii-partition-names.md` (foreman ruling at P3 run B): `xdu -o s3://`
  refuses a top-level directory with a non-ASCII name, because object_store percent-encodes it
  in a key; building keys raw would admit such names, but needs a reader-parity wire test first,
  since httpfs's handling of non-ASCII keys is unprobed.
  *Run A:* the walk of P1–P4's bodies, PLAN, GOAL and the branch's commits found no deferral
  beyond the six named here. Filed from the template, each with a `ROADMAP.md` entry:
  `issues/s3-index-rm.md`, `issues/s3-credential-chain.md`, `issues/s3-proxy-support.md`,
  `issues/s3-non-ascii-partition-names.md`, `issues/local-reader-bound-parameters.md` (which also
  carries P4's two local-SQL findings: a `'` in a directory name breaks a local `xdu-view`
  listing, and the list and tree directory statements can share one builder once local SQL is
  rewritten) and `issues/duckdb-debug-build-integral-assertion.md`. The S3 entries sit after `S3
  as a crawl source`; the engine one sits after the macOS SDK entry. The `S3 as an index target`
  entry now points at `spec/s3-index-target/` and its deferrals; its seed keeps its `shaped`
  status until the cycle lands. Before the ledger, `origin/main` (`a0cdd95`, PR #32) was merged
  into the branch; it applied cleanly.
- [x] If P4 was cut by amendment, record `issues/s3-index-view.md` and its ROADMAP entry here.
  *Run A:* not cut; nothing to record.
- **Run B evidence:** test legs as above with N ≥ 34 (the final `<min>` in the workflow equals the
  listed count); the Packaging job's literal step green on all four pages. *Recorded by run B in
  [`evidence/p5-ci.md`](evidence/p5-ci.md) (tests run 42040878700 on merge ref `506097e`, whose
  tree is `b6673d3`'s; N = 41 on both legs, equal to the workflow's minimum; the literal step
  green on all four pages).*
- **Verify:** see frontmatter.
- **Touches:** `issues/`, `ROADMAP.md`, `.github/workflows/test.yaml`, `AGENTS.md`, `doc/*.scd`.

> **Amendment (P5, review cycle 1 remediation):** F2, F6, the manual half of F5, and the region
> sentence on the three reader/writer man pages (triage above). Gate retuned: the workflow's
> minimum must equal the file's listed count and the whole file must pass at it; all five path
> lists name `src/s3.rs`, `src/view_query.rs` and `src/engine.rs` (AGENTS.md's within its
> High-risk section); AGENTS.md Testing names the script, `--include-ignored`,
> `XDU_TEST_EXTENSION_DIR` and the fetch; §2b names the S3 symbols; the CLI surface shows `s3://`.
>
> - [x] Man pages: scdoc is not installed here, so the region sentence was not rendered. It adds
>   no asserted literal (no `AWS_*`, `XDU_*` or `s3://` text) and no line opens with `.` or `'`.
> - [ ] Run B: evidence below.
>
> **Run B evidence (cycle 1):** both test legs' `s3-wire: '' 43 of 43 passed`, equal to the
> workflow's minimum; the Packaging job's "OK: every asserted literal survived into the published
> man-page text"; the Lint job green. Record in `evidence/p5-ci.md`.

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
