# REVIEW — Build an index into S3 and query it in place

> Adversarial QA by `xdu-review`, run in an isolated/clean context. The correctness pass grades the
> branch diff against [`GOAL.md`](GOAL.md) + the AGENTS.md invariants **only** — it does not see
> `PLAN.md`/`TECH.md` (avoids grading-its-own-homework / plan-sycophancy). Every finding cites an
> **executed** command, not an assertion.

- **Reviewed commit:** 435239ce522752a6f894ac30f0aa237354b9e3e4  ·  **Base:** `origin/main` (merge-base `a0cdd95`)  ·  **Date:** 2026-11-05
- **Verdict:** changes-requested
- **Cycle:** 1 of ≤3 — mirrors `review.cycle` in `TECH.md` (escalate to human on non-convergence)

## How this review ran

- **Mode:** the `debate` variant, because the diff touches the coupled core: the crawl writer, reader
  SQL, `xdu-view`, invariants §2, §2b, §2c and §5, the dependency tree, and secrets handling. Two
  fresh blind reviewers ran independently, A instructed to argue "ship" and B to argue "block".
  Neither saw the other's output.
- **What the reviewers saw:** `GOAL.md` inline; the spec-excluded diff
  (`git diff origin/main...HEAD -- . ':(exclude)spec/'`) and `git log --oneline`; `invariants.md`
  at HEAD and at base, `review-rubric.md` and `AGENTS.md`, read in full from the repo; the runnable
  worktree. They were barred from `PLAN.md`, `TECH.md`, `research/`, `META.md`, `evidence/`, the
  branch's commit bodies, and the build's scratch under `target/`. One exception was granted:
  `spec/s3-index-target/verify/wire.sh`, because `.github/workflows/test.yaml` runs it as the CI S3
  gate (see the meta-note).
- **Contract drift:** `git log --oneline origin/main..HEAD -- spec/s3-index-target/GOAL.md` lists
  `e4ba7d0` (shaping), `e3593e9` (GOAL-gate rulings) and `0f0edbd` (PLAN-gate amendment: R1's
  unsafe-name refusal, R4's reader-only absent prefix, R5/R6's bound `CREATE SECRET`). The gate
  judge made both amendments under the maintainer's standing mandate. They count as the human
  confirmation this check asks for, and the review grades the GOAL as it stands.
- **Artifact-deliverable R-IDs:** none. Every R-ID has code, tests, docs or workflow text in the
  spec-excluded diff. The orchestrator owns only the CI-run halves of R3 ("on every CI run") and
  R10 ("CI SHALL provide that endpoint"), graded from the CI logs below.

## Verification run

Commands executed and their outcomes. "A" and "B" are the blind reviewers; "O" is the orchestrator.

- `cargo test --locked --all-features` (A, B) → green. `s3_tests` reports `2 passed; 0 failed; 39
  ignored`, so the wire tier reports itself skipped instead of passing.
- `XDU_TEST_EXTENSION_DIR=target/test-ext XDU_TEST_S3_REQUIRED=1 cargo test --locked --test s3_tests
  -- --include-ignored` (A, B) → `41 passed`, equal to the 41 that `-- --list --include-ignored`
  lists. Includes `s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%)`.
- `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets --all-features -- -D
  warnings` (A, B) → exit 0.
- **CLI drives (A, B)** against throwaway loopback S3 servers, one per Bash call, each started and
  stopped inside that call. A wrote a minimal path-style server with switchable failure modes. B
  built a standalone server from the pinned `s3s`/`s3s-fs` crates.
  - Local and S3 parity for `--count`, `--top`, `-f csv`, sorted output and `-u`, over 13 or more
    awkward partition names (space, `+`, `'`, `,`, `&`, `$`, `:`, `@`, `...`, `.hidden`).
  - Refusals of `*`, `[`, `{`, `%`, `"`, `<`, control characters and non-ASCII names. Each exits 1
    with zero PUTs or DELETEs, and the index stays byte-identical.
  - Re-run prune past gaps, with listings paged at 2 keys.
  - Read-only credentials leave the marker in place.
  - The failure modes print a named cause and no stdout.
  - `xdu-rm` refuses through both `-i` and `XDU_INDEX`.
  - The missing-extension error names the path and both remedies.
  - `HOME`, `TMPDIR` and the working directory stay empty after a write and a read.
- **The `xdu-view` TUI was not driven interactively.** This host refuses a pseudo-terminal
  (`openpty: Operation not permitted`). Its R2 part rests on the `view_*` wire tests over
  `lib::view_query`.
- **Findings 1 and 3 re-run by O:** O re-ran reviewer B's probes for findings 1 and 3, and both
  reproduced (evidence under each finding).
- **CI on the merge ref (O), Tests run on `506097e`** (`b6673d3` merged into `a0cdd95`; `435239c`
  differs from `b6673d3` only in `spec/`, per `git diff --stat b6673d3 435239c`). Both legs,
  `ubuntu-24.04` and `macos-15`, ran the `S3 tests` step:
  - `test result: ok. 41 passed; 0 failed`, then `s3-wire: '' 41 of 41 passed`.
  - `s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD
    10, GET 9`.
  - Linux printed `write-unsafe-names: 5 case(s), non-UTF-8 case ran`. macOS skipped the non-UTF-8
    case, naming the filesystem's refusal.

  The `Lint` and `Packaging` jobs logged no `##[error]`. Packaging printed `OK: every asserted
  literal survived into the published man-page text`, which comes from CI's distro `scdoc` and its
  counting `check` form. The Release run (built at `b6673d3`, 5 jobs) and the Publish Image run
  (2 validate jobs) logged zero `##[error]`.
- **Man-page render (local): unavailable.** `scdoc` is not installed on this host, and this
  session installs nothing. The published text was not read here.
  - What was checked instead: by inspection, A and B compared each binary's `--help` flag set with
    its `.scd`, and found no new flags and an `-o` text that matches. Both checked the R9 literals
    and the scdoc markup rules: no line opens with `.` or `'`, and `\*` is escaped.
  - A also ran a source-level normalizer, which reproduced every count in CI's new `check` lists
    for all four pages.
  - **This is an unclosed gap for the human.** CI's count gate is green, but no one read the
    rendered text. `AGENTS.md`'s Commands section has the one-line `scdoc` install and the reading
    form.
- `git status --porcelain` after both hand-backs (O) → empty. Neither reviewer moved, replaced or
  instrumented anything under `target/`, so no build state needed restoring.
  - `target/test-ext/httpfs.duckdb_extension` is unchanged, and `target/lib/` does not exist.
  - B built one separate `CARGO_TARGET_DIR` for its server, under its scratch.

## Requirement → evidence matrix

| R-ID | Implemented by | Verified how (who) | Status |
|------|----------------|--------------------|--------|
| R1 | `s3::S3Sink` (`preflight`, `check_top_level`, `clear_marker`, `put_chunk`, `finalize_partition`, `write_marker`), `s3::check_s3_partition_names`, `crawl::IndexSink`/`LocalSink`/`encode_chunk`, `bin/xdu.rs` ordering | `write_*` wire tests; parity, refusal and prune drives (A, B); CI both legs (O) | ✅ |
| R2 | `lib::parquet_source`, `index_glob_s3`, `read_completion_marker_s3`, `marker_version_error`/`marker_completion_warning`, `view_query`, `bin/xdu-find.rs`, `bin/xdu-view.rs` | `find_*`/`view_*` wire tests, CLI parity drives (A, B); TUI not driven | ✅ (endpoint coherence: finding 1) |
| R3 | httpfs ranged reads; the wire test's whole-object-GET assertion | `find_count_receives_under_five_percent_of_stored_bytes`, 0.824% locally (A, B) and on both CI legs (O) | ✅ |
| R4 | `S3Sink::request_error`, `lib::s3_request_error`, pre-flight LIST | `*_failures_*`, read-only and refused-chunk wire tests; CLI drives (A, B) | ✅ |
| R5 | `S3Settings::resolve`, `S3Settings::duckdb_secret_statement`, `engine::open_s3_connection_from` | Unit tests, `harness_*` tests, probes (A, B, O) | ❌ findings 1, 3, 4 |
| R6 | `parquet_source` binds the glob; `view_query` binds `path LIKE ?`; marker URLs bound | `test_parquet_source_keeps_local_sql_byte_for_byte`, `find_prefix_with_a_quote_matches_local`, `view_quotes_in_the_prefix_and_a_directory_match_local`; `-u "o'brien"` over S3 (A, B) | ✅ |
| R7 | `engine::load_extension_from` error text, `open_s3_connection` | `s3_index_without_httpfs_names_the_path_and_both_remedies`, `local_tools_ignore_a_garbage_httpfs`, `offline_tests` 1 passed (A, B) | ✅ |
| R8 | early refusal in `src/bin/xdu-rm.rs:28-33` | `rm_tests` case; `env -i … XDU_INDEX=s3://b/p xdu-rm --force` prints one line and exits 1 (A, B) | ✅ |
| R9 | `doc/xdu.1.scd`, `doc/xdu-find.1.scd`, `doc/xdu-view.1.scd`, `doc/xdu-rm.1.scd`; README "Indexes in S3" | By inspection and source-level counts (A, B); CI count gate green (O); **render not read locally** | ✅ (gap noted) |
| R10 | in-process `s3s` endpoint and counting proxy in `tests/common/s3.rs`; `#[ignore]`; workflow fetch step plus `wire.sh "" 41` | 41/41 locally (A, B); 41/41 on both CI legs; plain `cargo test` reports 39 ignored (O) | ✅ |

Unmapped changes (none blocking):

- The README's `-b 100000` becomes `-B 100000`, an unrelated doc typo.
- Three deferral seeds beyond the GOAL's three, each with a `ROADMAP.md` entry:
  `issues/duckdb-debug-build-integral-assertion.md`, `issues/s3-non-ascii-partition-names.md`,
  `issues/s3-proxy-support.md`. They are deferral records, as `AGENTS.md` asks.
- A status line in `issues/s3-index-target.md`.
- `wire.sh` exports `XDU_TEST_S3_REQUIRED`, which nothing reads.

## Findings

Severity follows the rubric. Six findings, all CONFIRMED. Findings 2 and 4 were raised by both
reviewers independently. No finding is an `invariants.md` §1–§12 violation, so none is
auto-CRITICAL.

### [HIGH/CONFIRMED] 1. A reader's endpoint falls back to `DUCKDB_S3_ENDPOINT` when the resolver picks AWS
- **Where:** `src/s3.rs:383-407` (`S3Settings::duckdb_secret_statement` adds `ENDPOINT ?` only when
  an endpoint was resolved), through `engine::open_s3_connection_from`.
- **Failure scenario:**
  - When nothing in R5's sources names an endpoint, the `CREATE SECRET` carries no `ENDPOINT`.
    DuckDB then reads its `s3_endpoint` setting, which httpfs fills from `DUCKDB_S3_ENDPOINT` when it
    loads.
  - So an operator with that variable set for other DuckDB work gets this split: `xdu -o
    s3://bucket/idx` writes to AWS, while `xdu-find`/`xdu-view -i s3://bucket/idx` read from that
    other host. The readers send it SigV4-signed requests, with the session token, if any.
  - This is the writer–reader coherence risk the GOAL names. It also falsifies the doc comment at
    `src/s3.rs:381-382`, which says the secret "outranks whatever `httpfs` read from `AWS_*` or
    `DUCKDB_S3_*`". `harness_stray_duckdb_s3_environment_does_not_redirect_the_connection` covers
    only the case where an endpoint is bound.
- **Evidence:** reviewer B's probe, re-run by the orchestrator. It uses a loopback raw-TCP listener
  on port `LP`, and bucket `127.0.0` with endpoint `1:LP`, which virtual-host addressing joins into
  `127.0.0.1:LP`.
  - Control, `env -u AWS_ENDPOINT_URL xdu-find -i s3://127.0.0/idx --count`: "cannot reach the S3
    endpoint". The listener logged 0 connections.
  - The same command with `DUCKDB_S3_ENDPOINT=1:LP`: `S3 request failed … SSL connection failed
    error for HTTP GET to '/?encoding-type=url&list-type=2&prefix=idx%2F'`. The listener logged 4
    connections, each opening with a TLS ClientHello (`\x16\x03\x01…`).
  - `xdu SRC -o s3://127.0.0/idx` under the same environment: 0 new connections, and "cannot reach
    the S3 endpoint".
- **Touches:** R5 ("one resolver … SHALL serve both the writer and the readers"; "an endpoint
  override from those sources"); the GOAL's resolver-coherence risk.

### [HIGH/CONFIRMED] 2. The high-blast-radius path lists were not widened to `src/s3.rs`
- **Where:**
  - `.agents/factory/invariants.md` *High-blast-radius files*, the authoritative copy;
  - `.agents/factory/review-rubric.md` *Mandatory human sign-off gate*;
  - `.agents/skills/xdu-review/SKILL.md` *Safety Principles*;
  - `.agents/factory/templates/REVIEW.md` *Human-gate triggers*;
  - `AGENTS.md` *High-risk files & footguns*.
- **Failure scenario:**
  - `src/s3.rs` now holds the S3 side of §2 (one PUT per chunk; `finalize_partition`'s LIST and
    DELETE prune past gaps) and of §2b (the marker DELETE as the first write, the marker PUT last).
    It also holds the S3 top-level-name refusal (`check_s3_partition_names`) and the one credential
    resolver. These are the duties that put `src/crawl.rs` and `src/bin/xdu.rs` on the list.
  - `src/view_query.rs` now holds every `xdu-view` statement on the §5 surface.
  - None of the five lists names either file. A CONFIRMED §2b, name-refusal or credential defect in
    `src/s3.rs` (findings 1, 3 and 4 among them) therefore does not trip the mandatory human gate by
    path.
  - The rubric says a list "may only ever widen" to follow logic to its new home.
- **Evidence:** `grep -rln "src/s3.rs\|view_query" .agents/ AGENTS.md` (A, B, O) matches only
  repo-map and §5 prose, never a path list. `git diff --stat origin/main...HEAD -- .agents/` touches
  only `invariants.md`.
- **Touches:** operating-manual drift that degrades a gate (rubric scope item 5; HIGH by the rubric).

### [MEDIUM/CONFIRMED] 3. A comment after a profile section header merges that profile into the previous one
- **Where:** `src/s3.rs:585-603` (`Ini::parse`). A header counts only when the trimmed line ends
  with `]`.
- **Failure scenario:**
  - In a shared credentials file, `[other] # second account` is not taken as a section header. The
    keys under it overwrite the `default` section's keys.
  - With no `AWS_PROFILE`, both tools then sign with the other account's key pair. `AWS_PROFILE=other`
    reports that no file defines the profile.
  - Python's `configparser`, which AWS's Python tooling uses, reads the same file as sections
    `['default', 'other']`, with default key `revb-key`.
- **Evidence:** reviewer B's probe, re-run by the orchestrator.
  - Plain file: `xdu-find -i s3://revb/idx --count` prints `6` and exits 0.
  - Commented header: the same command fails with "the S3 endpoint refused the credentials … (HTTP
    403)" and exits 1.
  - Commented header with `AWS_PROFILE=other`: "names profile 'other', which neither … defines".
  - `xdu` with the commented file: "refused the credentials". The server's log shows the
    unregistered `other-key` was sent.
- **Touches:** R5 (credentials from the shared files, selected by `AWS_PROFILE`).

### [MEDIUM/CONFIRMED] 4. The writer prints the endpoint's error body, which can echo the session token
- **Where:** `src/s3.rs:850-877` (`S3Sink::request_error` appends `({err})`). object_store's
  Display for a non-2xx response includes the response body.
- **Failure scenario:**
  - With temporary credentials and a `SignatureDoesNotMatch` (a wrong secret, or a middlebox
    rewriting a header), AWS's error body echoes `<CanonicalRequest>`, signed header values
    included. One of those is `x-amz-security-token`.
  - `xdu -o s3://…` then prints the session token to stderr and to any non-TTY log.
  - The reader does not: `lib::s3_request_error` drops DuckDB's text on 401/403.
  - The doc comment at `src/s3.rs:848-849` says "No credential is ever part of the text".
- **Evidence:** reproduced on the client side. Reviewer A ran `env -i … AWS_SESSION_TOKEN=TOKENsecretABC987
  target/debug/xdu SRC -o s3://b/idx` against a loopback server in a mode that answers 403 with an
  AWS-shaped body echoing the signed headers.
  - The writer exited 1, and its stderr contains `x-amz-security-token:TOKENsecretABC987` once.
  - The reader against the same server printed only the HTTP 403 cause, with 0 occurrences.
  - Reviewer B saw the writer print the whole `SignatureDoesNotMatch` XML body from `s3s`, whose body
    carries no canonical request.
  - Whether a given provider echoes the canonical request lies outside the loopback acceptance.
    AWS's documented error shape does.
- **Touches:** R5 ("No secret SHALL appear … in any output").

### [MEDIUM/CONFIRMED] 5. CI's S3 gate lives under `spec/`, which `invariants.md` calls a record, not a gate
- **Where:**
  - `.github/workflows/test.yaml` runs `sh spec/s3-index-target/verify/wire.sh "" 41`;
  - `.agents/factory/invariants.md:273-274`, in §13's literal-count bullet, says a
    `spec/{slug}/verify/` harness "is a record once the spec merges, not a gate";
  - `AGENTS.md` Testing and Commands do not mention the S3 wire tier.
- **Failure scenario:**
  - After merge, a standing CI gate sits in a design-record directory, with its minimum count kept by
    hand in the workflow. The manual tells the next cycle to treat it as a record.
  - The pre-release gate `AGENTS.md` documents, `cargo test`, runs 2 of the 41 S3 tests. Nothing
    tells a contributor about `XDU_TEST_EXTENSION_DIR`, the `#[ignore]` tier or `--include-ignored`.
- **Evidence:** read of the workflow step and of `invariants.md:274` (B, O). `cargo test --locked`
  gives `s3_tests … 2 passed; 0 failed; 39 ignored` (B).
  `grep -nE "s3_tests|include-ignored|XDU_TEST_EXTENSION_DIR" AGENTS.md` returns no match (O).
- **Touches:** operating-manual drift (rubric scope item 5).

### [MEDIUM/CONFIRMED] 6. §2b and the AGENTS.md CLI surface describe the S3 path imprecisely
- **Where:**
  - `.agents/factory/invariants.md:86` says "All three readers call `lib::index_version_error`
    before `lib::index_completion_warning`". The S3 readers call `marker_version_error`/
    `marker_completion_warning` over `read_completion_marker_s3`.
  - `invariants.md:89`'s size cap does not describe the S3 bound: a HEAD for the size, then a
    refusal over `MARKER_READ_LIMIT`.
  - `AGENTS.md` *CLI surface* still gives `xdu … -o/--outdir DIR` and `-i/--index DIR`, with no
    `s3://`.
- **Failure scenario:** the rule holds in code: the version gate comes before the warning, and an
  absent marker means refusal. But the map names the local functions only, so a later reviewer
  checking the S3 path against §2b checks the wrong symbols.
- **Evidence:** read of `invariants.md` §2b and `AGENTS.md` *CLI surface* against `src/lib.rs`
  `read_completion_marker_s3` and `marker_version_error` (A, B, O).
- **Touches:** operating-manual drift (rubric scope item 5).

**Considered and dropped:**

- **The marker read can exceed the cap** if the marker object grows between the HEAD and the GET.
  A reproduced this, but the local reader has the same stat-then-read window, and it needs a hostile
  concurrent writer.
- **One `-u bob -f size` order differed** between local and S3. Ties under `ORDER BY size` reorder
  on local runs alone, and the sorted outputs match.
- **`+` in listings.** httpfs decodes it.
- **The cost of the `**` listing** on scoped queries. It is tiny against stored bytes, and
  throughput tuning is a non-goal.
- **Proxy variables.** Both directions ignore them.

## Human-gate triggers

- **By the letter of the current lists: not triggered.** No CONFIRMED finding lands in a listed core
  path. Findings 1, 3 and 4 are in `src/s3.rs` and `src/engine.rs`, and none breaks §1, §2, §4 or §5
  as written. No credential becomes SQL text.
- **By the widening the rubric requires: triggered.** Finding 2 shows that those lists omit
  `src/s3.rs`, which now carries §2/§2b sequencing and the credential resolver. Findings 1, 3 and 4
  are in it, and the rubric lets a list only widen to follow its logic.
- **Recommendation:** treat the gate as applying, and get explicit human sign-off before this
  branch goes on to `xdu-publish`.

## Optional completeness sub-pass (separate reviewer; may see TECH.md)

Not run (no `completeness` argument).

## Review cycle 2 — changes-requested (2026-11-05)

- **Reviewed commit:** ef4fedfb859756bb5a52f1461cd7cb5e48e9aeea  ·  **Base:** `origin/main`
  (merge-base `a0cdd95`)  ·  **Date:** 2026-11-05
- **Verdict:** changes-requested
- **Cycle:** 2 of ≤3, mirroring `review.cycle` in `TECH.md`. This is the second cycle without
  convergence, so the skill escalates to the human.

### How this review ran

- **Mode:** a fresh blind pass over the full spec-excluded diff, which is the default for a later
  cycle. The foreman asked for the whole change to be graded against the GOAL, not only the
  remediation. It used the `debate` variant again, for the same reason as cycle 1: the diff
  touches the coupled core. Reviewer A argued "ship" and B argued "block". Neither saw the
  other's output, nor cycle 1's findings.
- **What the reviewers saw:**
  - `GOAL.md`, `invariants.md` and `review-rubric.md`, verbatim at `ef4fedf`, in one brief
    file. The orchestrator built that file with `cat` from the committed files instead of
    retyping about 690 lines twice (meta-note F9).
  - The spec-excluded diff, `git log --oneline`, `AGENTS.md`, and the runnable worktree.
  - They were barred from all of `spec/`, which covers cycle 1's `REVIEW.md` and `TECH.md`'s
    remediation triage. They were also barred from commit bodies, `target/ci/`, and build
    scratch they did not create.
  - Cycle 1's one exception, `wire.sh`, was not needed: the CI gate now lives in
    `tests/s3-wire.sh`.
- **One correction mid-run.** The orchestrator's brief gave the relative
  `XDU_TEST_EXTENSION_DIR=target/test-ext` that `AGENTS.md` documents. Once the orchestrator had
  reproduced that form failing (finding 2), it told both reviewers to use an absolute path, and
  said the behaviour was already recorded. By then A had reproduced it on its own.
- **Contract drift:** the same three GOAL commits as cycle 1 (`e4ba7d0`, `e3593e9`, `0f0edbd`),
  and none since. The gate judge confirmed them under the maintainer's standing mandate, so this
  cycle grades the GOAL as it stands.
- **Artifact-deliverable R-IDs:** none. The orchestrator owns the CI-run halves of R3 and R10,
  graded from the CI logs below.

### Verification run

"A" and "B" are the blind reviewers; "O" is the orchestrator.

- `sh tests/s3-wire.sh "" 43` with `XDU_TEST_EXTENSION_DIR` unset, so the script's absolute
  default applies (O) → exit 0:
  - `s3-wire: '' 43 of 43 passed`
  - `s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD
    10, GET 9`
  - `write-unsafe-names: 4 case(s), non-UTF-8 case skipped: the filesystem refuses the name`
    (APFS)
- `XDU_TEST_EXTENSION_DIR=target/test-ext sh tests/s3-wire.sh "" 43`, the form `AGENTS.md`
  documents (O) → exit 1 with `22 passed; 21 failed`. Every failure reads `DuckDB extension
  'httpfs' not found at target/test-ext/httpfs.duckdb_extension`. A reproduced this with `cargo
  test` directly. This is finding 2.
- `cargo test --locked --offline --test s3_tests -- --include-ignored` with an absolute extension
  directory (A, B) → `43 passed`, equal to the 43 that `-- --list --include-ignored` lists. The
  negative control printed `s3-scan-bytes: ... (90.271%)`.
- `cargo test --locked --offline` (A, B) → exit 0. lib passed 154 tests; `s3_tests` reported
  `2 passed; 0 failed; 41 ignored`.
- `cargo fmt --all -- --check` and `cargo clippy --locked --offline --all-targets --all-features
  -- -D warnings` (A, B) → exit 0.
- **CLI drives (A, B)** against loopback S3 servers they wrote. Each server started and stopped
  within one Bash call, and a connect to every port was refused afterwards.
  - **Parity.** Local and S3 gave the same results over 12 (A) and 17 (B) awkward partition names
    and 6 awkward prefixes (A), for `--count`, the default listing, `--top`, `-u` on every
    partition including `__root__`, `-f csv`/`-f json`, and sorted output.
    - Locally, `-u "it's"` fails with a DuckDB parser error. That is the recorded §5 exception for
      local paths, which the GOAL lists as a Non-goal. Over S3 the same query answers correctly.
  - **A partition scope with a size filter and a size sort, on the release build (B):** the same
    as local for 7 argument sets. The debug build aborts on the local query, which is the filed
    `issues/duckdb-debug-build-integral-assertion.md`.
  - **Error bodies that echo the secret or the session token** (A, B): 403
    SignatureDoesNotMatch, 400 InvalidToken, 400 ExpiredToken and 500. `xdu`, `xdu-find` and
    `xdu-view` each exit 1 with empty stdout, and neither the secret nor the token appears in any
    output.
  - **Every proxy variable pointed at a dead port** (A, B): the reader and the writer both
    succeed, as the man pages say.
  - **Re-run with fewer files** (A): the stale chunks are deleted. The request order is a LIST of
    the prefix, the marker DELETE, the chunk PUTs and partition LISTs, then the marker PUT last.
    The reader's marker access is HEAD, HEAD, then a bounded GET of `bytes=0-98`.
  - **Missing credentials, an empty `XDU_EXTENSION_DIR`, and `xdu-rm` through both `-i` and
    `XDU_INDEX`** (A, B): each names its cause, exits 1, prints no stdout and leaves `HOME`
    empty. `xdu-rm` sent 0 requests.
  - **`xdu-view` without a TTY** (B): it ran its S3 queries before terminal setup, every chunk
    read a ranged 206.
- **The `xdu-view` TUI was not driven interactively:** `openpty: Operation not permitted` (A, B).
- **Findings 3 and 4 re-run by O.** Both reproduced; the evidence is under each finding.
- **CI on the merge ref (O): Tests run 42058060960 on `0d80d6a`.** That is `fbecdb7` merged into
  `a0cdd95`. `a0cdd95` is an ancestor of `fbecdb7`, so the merge's tree is `fbecdb7`'s, and
  `ef4fedf` differs from `fbecdb7` only in four `spec/` files.
  - Both legs ran the `S3 tests` step: `test result: ok. 43 passed; 0 failed`, then `s3-wire: ''
    43 of 43 passed`, then `s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21
    requests: LIST 2, HEAD 10, GET 9`.
  - Linux ran the non-UTF-8 case (`write-unsafe-names: 5 case(s), non-UTF-8 case ran`). macOS
    skipped it, naming the filesystem's refusal.
  - The plain `Test` step reports `s3_tests` as `2 passed; 0 failed; 41 ignored` on both legs.
  - Packaging printed `OK: every asserted literal survived into the published man-page text`.
  - None of the three logs has a `##[error]` line: Tests, Release run 42058061145 (5 jobs, at
    `fbecdb7`) and Publish Image run 42058061014 (2 validate jobs).
  - O spot-checked the lines quoted in the "Review cycle 1" sections of `evidence/p1-ci.md`,
    `p3-ci.md` and `p5-ci.md` against the logs, and each matches.
- **Man-page render (local): unavailable again.** `scdoc` is not installed, and this session
  installs nothing. A and B approximated scdoc's markup at source level, and every count in CI's
  `check` lists matched. No `.scd` line opens with `.` or `'`. CI's distro-`scdoc` count gate is
  green, but the published text has still not been read locally. That remains an unclosed gap
  for the human (`AGENTS.md`'s Commands section has the install line).
- **Tree state (O).** `git status --porcelain` was empty after both hand-backs and after O's
  probes. Nothing mutated build state: `target/test-ext/httpfs.duckdb_extension` predates the
  review and `target/lib/` does not exist. O's two runs of `tests/s3-wire.sh` rewrote
  `target/wire-all.log` and `target/wire-all.list`, the script's fixed names.

### Cycle 1 findings at this head (O)

| Cycle 1 finding | Status at `ef4fedf` |
|---|---|
| 1. A reader's endpoint falls back to `DUCKDB_S3_ENDPOINT` | Closed. `harness_stray_duckdb_s3_endpoint_is_not_taken_when_the_resolver_picks_aws` passes locally and on both CI legs: the stray listener sees 0 connections, and every CONNECT names the regional AWS host. Neither blind reviewer found a fallback. |
| 2. Path lists not widened | Closed. All five copies name the eight paths. A and B found that the three restated copies match the `invariants.md` header exactly. |
| 3. A commented header merges profiles | Closed. `a_comment_after_a_section_header_keeps_the_profiles_apart`, `a_malformed_section_header_takes_no_keys_from_or_into_another_profile` and `section_headers_follow_the_shared_file_rules` pass. |
| 4. The writer echoes the session token | Closed. `write_refusal_bodies_that_echo_the_session_token_are_not_printed` passes. A's and B's probes found 0 occurrences across three tools and four bodies. |
| 5. CI's S3 gate under `spec/` | Closed. The gate is `tests/s3-wire.sh`, with a minimum of 43, and `AGENTS.md` documents it. The documented command is wrong, though: finding 2 below. |
| 6. §2b and the CLI surface | Closed in `invariants.md` §2b and in `AGENTS.md`'s CLI surface. `AGENTS.md`'s Architecture copy of the same sentence is still local-only: finding 1 below. |

### Requirement → evidence matrix

| R-ID | Implemented by | Verified how (who) | Status |
|------|----------------|--------------------|--------|
| R1 | `s3::S3Sink`, `check_s3_partition_names`; `crawl::IndexSink`/`LocalSink`/`encode_chunk`; ordering in `bin/xdu.rs` | `write_*` wire tests; parity, prune and request-order drives (A, B); CI both legs (O) | ✅ |
| R2 | `lib::parquet_source`, `index_glob_s3`, `read_completion_marker_s3`, `marker_*` gates; `view_query`; both reader bins | `find_*`/`view_*` wire tests; parity drives; ranged 206 reads (A, B). TUI not driven | ✅ |
| R3 | httpfs ranged reads; nothing staged | `find_count_receives_under_five_percent_of_stored_bytes` at 0.824%, with a 90.271% negative control (A, B, O); both CI legs at 0.824% (O) | ✅ |
| R4 | `S3Sink::request_error`, `lib::s3_request_error`, pre-flight LIST | `*_failures_*`, read-only and refused-chunk wire tests; missing-credential drives (A, B) | ✅ |
| R5 | `S3Settings::resolve`, `endpoint_authority`, `client_endpoint_url`, `duckdb_secret_statement`; `engine::open_s3_connection_from` | Unit tests, `harness_*`, profile and secret wire tests, echo probes (A, B, O) | ✅ with two LOW edge cases: findings 3 and 4 |
| R6 | bound glob, bound `path LIKE ?`, bound marker URLs | `test_parquet_source_*`, `local_view_sql_is_byte_for_byte_what_xdu_view_sent`, quote-prefix wire tests; `-u "it's"` over S3 (A, B) | ✅ |
| R7 | `engine::load_extension_from` error text; `open_s3_connection` | `s3_index_without_httpfs_names_the_path_and_both_remedies`, `local_tools_ignore_a_garbage_httpfs`, `offline_tests`; empty-directory drives (A, B) | ✅ |
| R8 | early refusal in `src/bin/xdu-rm.rs` | `rm_tests` case; `-i` and `XDU_INDEX` drives, 0 requests (A, B) | ✅ |
| R9 | the four `doc/*.scd` pages; README "Indexes in S3"; CI literal list | Source-level counts (A, B); CI count gate green (O); **render not read locally** | ✅ (gap noted) |
| R10 | in-process `s3s` endpoint, `#[ignore]` tier, `tests/s3-wire.sh`, CI step with a minimum of 43 | 43/43 locally (A, B, O) and on both CI legs (O); plain `cargo test` lists 41 ignored | ✅ (documented local form wrong: finding 2) |

**Unmapped changes**, none blocking: the same as cycle 1, minus the dead `XDU_TEST_S3_REQUIRED`
export, which is gone. The moves of `xdu-view`'s SQL into `src/view_query.rs` and of the local
write path behind `IndexSink` serve R1, R2 and R6 (A, B).

**A note, not a finding:** `xdu-find --help` and `xdu-view --help` still describe `-i` as a
directory (A, B). R9 asks only for the man pages and the README, both of which carry `s3://`, and
no page advertises a flag the binaries lack.

### Findings

Four findings, all CONFIRMED. Finding 1 was raised by both reviewers independently. None is an
`invariants.md` §1–§12 violation, so none is auto-CRITICAL.

#### [MEDIUM/CONFIRMED] 1. `AGENTS.md` describes only the local marker gate
- **Where:** `AGENTS.md:402-406`, in the "Run-level completion marker" bullet under
  Architecture. Load-bearing invariant 2 points at that bullet ("see Architecture").
- **Failure scenario:**
  - The bullet says "All three readers call `lib::index_version_error` first … via
    `lib::index_completion_warning`. Reads are capped (`MARKER_READ_LIMIT`) and non-blocking".
  - On an `s3://` index, `xdu-find` and `xdu-view` call neither function. They read with
    `lib::read_completion_marker_s3` (a LIST, then a HEAD), then call `lib::marker_version_error`
    and `lib::marker_completion_warning`, and refuse an oversized marker as
    `MarkerRead::Oversized` without fetching it.
  - `invariants.md` §2b already says this; cycle 1's finding 6 corrected it there. But
    `AGENTS.md` declares that it wins when the two drift, and the copy that wins is the narrower
    one.
- **Evidence:** a grep of `AGENTS.md` finds 0 matches for each of `read_completion_marker_s3`,
  `marker_version_error`, `marker_completion_warning` and `Oversized`. `invariants.md` has 1–2
  each (A, B). O read the bullet.
- **Severity:** B argued HIGH, since invariant 2 delegates the marker rule to this bullet. It is
  graded MEDIUM, as cycle 1's finding 6 was. The gate text that `xdu-plan` and `xdu-review` draw
  from, `invariants.md` §2b, is correct, so no gate is degraded.
- **Touches:** operating-manual drift (rubric scope item 5); §2b.

#### [MEDIUM/CONFIRMED] 2. The documented S3 wire-gate command fails 21 of its 43 tests
- **Where:**
  - `AGENTS.md:216` (Commands) and `AGENTS.md:578` (Testing) give
    `XDU_TEST_EXTENSION_DIR=target/test-ext sh tests/s3-wire.sh "" 43`.
  - `tests/common/s3.rs:64-79`: `test_extension_dir` checks the file relative to the test's own
    working directory, then returns the value unchanged.
  - `tests/common/s3.rs:345,356` pass that value to the spawned binaries as
    `XDU_EXTENSION_DIR`, and those binaries run in an empty temporary working directory
    (`:836`).
- **Failure scenario:**
  - A contributor who runs the gate exactly as the manual says sees 21 S3 tests fail with
    "httpfs not found". They are all the tests that hand the directory to a child.
  - The failure is loud, never a silent pass. CI is unaffected, because it passes the absolute
    `runner.temp` path.
  - The script's own default, `$PWD/target/test-ext`, works. So does any absolute path.
  - Cycle 1's finding 5 remediation introduced this text.
- **Evidence:**
  - O ran the documented command: exit 1, `test result: FAILED. 22 passed; 21 failed`, with
    `s3-wire.sh: cargo test exited 101`.
  - The same script with the variable unset: `43 of 43 passed`.
  - A reproduced the split with `cargo test` directly.
- **Touches:** operating-manual drift (rubric scope item 5): the local form of a standing CI gate.
  Also R10's local half.

#### [LOW/CONFIRMED] 3. A bucket name that carries `host:port#` moves the writer to that host
- **Where:** `src/s3.rs:100-134` (`S3Locator::parse` refuses only `@` and glob metacharacters in
  the bucket), `src/s3.rs:411-419` (`client_endpoint_url` builds
  `https://{bucket}.{authority}`), used at `src/s3.rs:830`.
- **Failure scenario:**
  - With no endpoint override, the writer's URL is `https://<bucket>.s3.<region>.amazonaws.com`.
  - A bucket of `127.0.0.1:PORT#` turns that into a URL whose authority is `127.0.0.1:PORT`. The
    rest of the intended host falls into the fragment.
  - The writer then sends its signed requests, session token included, to a host that none of
    R5's sources named.
  - For the same locator the reader fails DNS instead. The writer and the readers address
    different hosts, which is the coherence the resolver's `endpoint_authority` exists to
    guarantee.
  - A real S3 bucket name cannot hold `:` or `#`.
  - It is graded LOW because the operator types the locator and could name any endpoint through
    `AWS_ENDPOINT_URL` anyway. No trust boundary is crossed.
- **Evidence:**
  - A's probe, re-run by O. A loopback TCP recorder listened on an ephemeral port; then this
    ran: `env -i PATH=… HOME=<empty> AWS_ACCESS_KEY_ID=k AWS_SECRET_ACCESS_KEY=s
    AWS_SESSION_TOKEN=tok AWS_REGION=us-east-1 target/debug/xdu SRC -o
    's3://127.0.0.1:PORT#/idx'`.
  - `xdu` exited 1 with "cannot reach the S3 endpoint while listing the index prefix … Error
    performing GET https://127.0.0.1:PORT/?list-type=2&prefix=idx%2F". (In O's run the `s` of
    `https` printed as `<redacted>`, because the probe's one-character secret was struck from
    the text.)
  - The recorder logged 4 connections, each opening with a TLS ClientHello (`160301`): the
    first try and three retries.
  - The same locator through `xdu-find -i` made no connection (A).
  - That the token would reach a host with a valid certificate is inferred: the recorder ended
    each handshake, so no request was sent.
- **Touches:** R5 ("one resolver … SHALL serve both the writer and the readers"; the endpoint
  comes from R5's sources).

#### [LOW/CONFIRMED] 4. A non-UTF-8 locator echoes the credentials written before `@`
- **Where:** `src/s3.rs:101-103`. The UTF-8 check prints the locator, and it runs before the `@`
  check at `src/s3.rs:109-116`. That check's comment says it must come "before anything that
  would print the locator: the part before `@` may be a secret".
- **Failure scenario:**
  - A locator that carries `KEY:SECRET@` and also one byte that is not UTF-8 is refused by the
    UTF-8 check, and that refusal prints the secret to stderr.
  - `xdu`, `xdu-find` and `xdu-view` are affected. `xdu-rm` is not, because it refuses on the
    scheme first.
  - The secret is one the operator typed on their own command line, and the input is an edge
    case.
- **Evidence:**
  - B's probe, re-run by O. `env -i PATH=/usr/bin:/bin HOME=<empty> target/debug/xdu-find -i
    $'s3://AKIDEXAMPLE:topsecret-Zq9@bucket/p\xff' --count` printed `Error: S3 locator is not
    valid UTF-8: s3://AKIDEXAMPLE:topsecret-Zq9@bucket/p�` and exited 1.
  - The control, the same locator without `\xff`, printed the non-echoing "an S3 locator must not
    carry credentials before the bucket" message.
  - B saw the same echo from `xdu -o`.
- **Touches:** R5 ("No secret SHALL appear … in any output").

**Considered and dropped:**
- **Reader error bodies leaking the token:** DuckDB prints no response body (A, B).
- **Partition-name and prefix parity, `+` in listings:** all names matched (A, B).
- **Proxy bypass:** behaves as documented (A, B).
- **The race between the marker HEAD and GET:** the local reader has the same stat-then-read
  window, so this is parity, not a regression (B).
- **Small chunks fetched by one range that covers the whole object:** that is DuckDB reading the
  footer, and every chunk GET was ranged (B).
- **A region in the shared credentials file going unread:** no reference implementation was
  available here to confirm a divergence (B).
- **DNS threads outside §7's budget:** could not be measured, since `ps` is refused (B).
- **Redaction of a very short secret mangling the error text:** O saw it with a one-character
  probe secret. Real key material is far longer, so it is not a finding.

### Human-gate triggers

- **Triggered by path.** Findings 3 and 4 are CONFIRMED and sit in `src/s3.rs`, which is on the
  authoritative list. Neither breaks §1, §2, §4 or §5 as written: no credential becomes SQL text,
  and the marker ordering holds.
- **Bounded loop.** This is cycle 2 without convergence, with one cycle left in the ≤3 bound.
  The skill escalates to the human.
- **Recommendation: fix all four in one remediation run, with human sign-off on the `src/s3.rs`
  changes.** Each fix is small and local, and the fix is cheaper than a deferral seed:
  1. Bring `AGENTS.md`'s marker bullet in line with §2b.
  2. Make `test_extension_dir` return an absolute path, so the documented command works as
     written.
  3. Have `S3Locator::parse` refuse a bucket whose characters could change the URL's authority,
     naming the character. Add a wire test that drives a `#` locator against a loopback
     listener and expects 0 connections.
  4. Run the `@` check before the UTF-8 check, or leave the locator out of the UTF-8 error.

### Optional completeness sub-pass (separate reviewer; may see TECH.md)

Not run (no `completeness` argument).

## Review cycle 3 — changes-requested (2026-11-05)

- **Reviewed commit:** fdc32709a5dbc8c2d2cf4b9c1eeba7bc9768347a  ·  **Base:** `origin/main`
  (merge-base `a0cdd95`)  ·  **Date:** 2026-11-05
- **Verdict:** changes-requested
- **Cycle:** 3 of ≤3, mirroring `review.cycle` in `TECH.md`. This is the last cycle in the bound
  and the third without convergence, so the skill stops and escalates to the human.

### How this review ran

- **Mode:** a fresh blind pass over the full spec-excluded diff, graded against the GOAL as it
  stands, not only cycle 2's remediation. It used the `debate` variant for the third time,
  because the diff touches the coupled core. Reviewer A argued "ship" and B argued "block".
  Neither saw the other's output or any earlier cycle's findings.
- **What the reviewers saw:**
  - `GOAL.md`, `invariants.md` and `review-rubric.md`, verbatim at `fdc3270`, in one brief file
    built with `cat` under the review's scratch directory in `target/` (meta-note F9, seen
    again).
  - The spec-excluded diff (`git diff origin/main...HEAD -- . ':(exclude)spec/'`),
    `git log --oneline`, `AGENTS.md`, and the runnable worktree.
  - They were barred from all of `spec/` beyond the brief, from commit bodies, from `target/ci/`
    and from build scratch they did not create. They were told not to run `tests/s3-wire.sh`,
    whose fixed log names under `target/` the orchestrator's run owns, and to run the wire tier
    with `cargo test` into their own scratch instead.
- **Contract drift:** the same three GOAL commits as cycles 1 and 2 (`e4ba7d0`, `e3593e9`,
  `0f0edbd`), and none since. The gate judge confirmed them under the maintainer's standing
  mandate, so this cycle grades the GOAL as it stands.
- **Artifact-deliverable R-IDs:** none. The orchestrator owns the CI-run halves of R3 and R10,
  graded from the CI logs below.

### Verification run

"A" and "B" are the blind reviewers; "O" is the orchestrator.

- `XDU_TEST_EXTENSION_DIR=target/test-ext sh tests/s3-wire.sh "" 46`, the relative form
  `AGENTS.md` documents (O) → exit 0:
  - `s3-wire: '' 46 of 46 passed`
  - `s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD
    10, GET 9`
  - `write-unsafe-names: 4 case(s), non-UTF-8 case skipped: the filesystem refuses the name`
    (APFS)
- The `harness_` tier in the same relative form, five times in a row while both reviewers were
  building and testing on the same host (O) → 5 of 5 runs `7 passed; 0 failed`.
- `cargo test --locked --offline --test s3_tests -- --include-ignored` with an absolute extension
  directory (A, B) → `46 passed; 0 failed`. A's meter negative control printed
  `s3-scan-bytes: ... (90.271%)`. With the extension directory unset or empty, a wire test fails
  with a panic naming the cause (B).
- `cargo test --locked --offline` (A, B) → exit 0. lib passed 157 tests; `s3_tests` reported
  `4 passed; 0 failed; 42 ignored`.
- `cargo fmt --all -- --check` and `cargo clippy --locked --offline --all-targets --all-features
  -- -D warnings` (A, B) → exit 0. `cargo tree -e normal` holds no `openssl-sys`, `native-tls` or
  `aws-lc-sys` (A, B).
- **Negative controls (A).** Each was built from a `git archive` export in A's scratch, with its
  own `CARGO_TARGET_DIR`. Each mutated file was restored by copy and checked with `cmp`.
  - Moving `clear_marker` ahead of `check_top_level` made
    `write_unsafe_partition_name_is_refused_before_the_clear` fail.
  - Interpolating the S3 glob as SQL text made `find_prefix_with_a_quote_matches_local` and
    `view_quotes_in_the_prefix_and_a_directory_match_local` fail, while the plain parity test
    still passed.
- **CLI drives (A, B)** against loopback S3 stand-ins they wrote. Each server started and stopped
  within one Bash call, and a connect to every port was refused afterwards.
  - **Parity:** local and S3 matched for `--count`, `--top`, the default listing, `-f csv`, sorted
    output and every `-u NAME`. The names covered 9 (A) and 17 (B) awkward partition names plus
    `__root__`, odd prefixes, and a bucket-root index.
  - **Re-runs (A, B):** a 1205-chunk partition re-run down to 3 chunks pruned 1202 (A). A
    `-j 8 -B 7` run of 221 chunks re-run with `-B 50` left 37 keys, the same as local (B).
  - **Echo bodies and failure modes (A, B):** error bodies that echo the token printed 0
    occurrences. A dead endpoint, missing credentials and a missing extension each name their
    cause, exit 1, print no stdout and leave `HOME` empty.
  - **Proxies and `xdu-rm` (A, B):** dead proxy variables were ignored by both directions, as
    documented. `xdu-rm` refuses through both `-i` and `XDU_INDEX`.
  - **TLS (B, re-run by O):** finding 1 below.
- **Cycle 2's finding 3 and 4 probes, re-run by O at this head.** `xdu` and `xdu-find`, each given
  `s3://AKIDEXAMPLE:topsecret-Zq9@bucket/p\xff`, `s3://AKIDEXAMPLE:top/secret-Zq9@bucket/p` and
  `s3://127.0.0.1:9#/idx`, exit 1. The output holds 0 occurrences of the key or the secret, and
  names the credentials refusal or the bucket's `:`.
- **The `xdu-view` TUI was not driven interactively:** `openpty: Operation not permitted` (A, B).
  Its R2 part rests on the `view_*` wire tests and the pre-terminal refusal tests.
- **CI (O): Tests run 42072876646 on the merge ref `f48b4f0`.** That is `40a9813` merged into
  `a0cdd95`. `a0cdd95` is an ancestor of `40a9813`, so the merge's tree is `40a9813`'s.
  `fdc3270` differs from `40a9813` only in `spec/`, `ROADMAP.md` and
  `issues/s3-legacy-bucket-names.md` (`git diff --stat 40a9813 fdc3270`), and no workflow reads
  either of the last two.
  - Both legs ran the `S3 tests` step: `test result: ok. 46 passed; 0 failed`, then `s3-wire: ''
    46 of 46 passed`, then the same `s3-count-bytes` line at 0.824%. Linux ran the non-UTF-8 case
    (`5 case(s), non-UTF-8 case ran`). macOS skipped it, naming the filesystem's refusal.
  - The plain `Test` step reports `s3_tests` as `4 passed; 0 failed; 42 ignored` on both legs,
    and both always-run tests that cycle 2 added pass there.
  - Packaging printed `OK: every asserted literal survived into the published man-page text`.
  - These logs have no `##[error]` line: Tests 42072876646, Release 42072876624 (5 jobs, at
    `40a9813`), Publish Image 42072876756 (2 validate jobs), and Publish Image 42094256569 (2
    validate jobs, on `fdc3270`'s merge ref `7cd747e`).
  - **Not observed:** a Tests run on `fdc3270` itself. No log of one was available.
  - O checked all 40 timestamped lines quoted under "Review cycle 2" in `evidence/p1-ci.md` and
    `p5-ci.md` with `grep -F` against the three logs, and every one matched.
- **Man-page render (local): unavailable again.** `scdoc` is not installed, and this session
  installs nothing. A and B checked the `.scd` sources against CI's `Nx:` counts at source level,
  and the counts matched. No line opens with `.` or `'`. CI's distro-`scdoc` count gate is green,
  but the published text has still not been read locally. That remains an unclosed gap for the
  human (`AGENTS.md`'s Commands section has the install line).
- **Tree state (O).** `git status --porcelain` was empty after both hand-backs and after O's
  probes. `target/test-ext/httpfs.duckdb_extension` is unchanged and predates the review.
  `target/lib/` does not exist. O's run of `tests/s3-wire.sh` rewrote `target/wire-all.log` and
  `target/wire-all.list`, the script's fixed names.
  - The review's scratch is under `target/review3-tmp/`, which is git-ignored, and it could not
    be removed: `del` fails there with "Operation not permitted". A's directory reports 36G
    apparent size. That is mostly a copy-on-write clone of `target/debug`, and free space fell by
    about 1 GiB.

### Cycle 2 findings at this head (O)

| Cycle 2 finding | Status at `fdc3270` |
|---|---|
| 1. `AGENTS.md` describes only the local marker gate | Closed. Architecture names `read_completion_marker_s3`, `marker_version_error`, `marker_completion_warning` and `MarkerRead::Oversized`. A checked the named symbols against the code. |
| 2. The documented wire-gate command fails 21 of 43 | Closed. O ran the documented relative form: `46 of 46 passed`. |
| 3. A `host:port#` bucket moves the writer | Closed. `S3Locator::parse` holds the bucket to S3's naming rules. `a_bucket_that_names_a_host_and_port_reaches_no_host` passes locally and on both CI legs. O's probe was refused naming the `:`. |
| 4. A non-UTF-8 locator echoes the credentials | Closed. The `@` check runs first, on the bytes. `a_locator_carrying_credentials_is_refused_without_printing_them` passes on both CI legs. O's three probes leaked 0 times. |
| 5. Flaky proxy count (foreman-observed) | Closed as far as can be observed. `harness_a_connection_that_carries_nothing_is_not_a_request` passes on both CI legs, and `harness_` passed 5 of 5 under concurrent load (O). |

### Requirement → evidence matrix

| R-ID | Implemented by | Verified how (who) | Status |
|------|----------------|--------------------|--------|
| R1 | `s3::S3Sink`, `check_s3_partition_names`; `crawl::IndexSink`/`LocalSink`; ordering in `bin/xdu.rs` | `write_*` wire tests; parity, prune and pagination drives (A, B); clear-order negative control (A); CI both legs (O) | ✅ |
| R2 | `lib::parquet_source`, `index_glob_s3`, `read_completion_marker_s3`, `marker_*` gates; `view_query`; both reader bins | `find_*`/`view_*` wire tests; parity drives, no whole-object chunk GET (A, B). TUI not driven | ✅. Finding 1 leaves the results open to forgery by an interceptor |
| R3 | httpfs ranged reads; nothing staged | `find_count_receives_under_five_percent_of_stored_bytes` at 0.824%, with a 90.271% negative control (A, B, O); both CI legs at 0.824% (O) | ✅ |
| R4 | `S3Sink::request_error`, `lib::s3_request_error`, pre-flight LIST | `*_failures_*`, read-only and refused-chunk wire tests; dead-endpoint and echo drives (A, B) | ✅ |
| R5 | `S3Settings::resolve`, `duckdb_secret_statement`; `engine::open_s3_connection_from` | Unit tests, `harness_*`, profile and secret wire tests, echo probes (A, B); locator probes (O) | ✅ for the resolver. Finding 1: one resolver feeds both directions, but only the writer verifies the host it reaches |
| R6 | bound glob, bound `path LIKE ?`, bound marker URLs | Unit pins; quote-prefix wire tests; glob-as-text negative control (A); `it's` over S3 (A, B) | ✅ |
| R7 | `engine::load_extension_from` error text; `open_s3_connection` | `s3_index_without_httpfs_names_the_path_and_both_remedies`, `local_tools_ignore_a_garbage_httpfs`, `offline_tests`; empty-directory drives (A, B) | ✅ |
| R8 | early refusal in `src/bin/xdu-rm.rs` | `rm_tests` case; `-i` and `XDU_INDEX` drives (A, B) | ✅ |
| R9 | the four `doc/*.scd` pages; README "Indexes in S3"; CI literal list | Source-level counts (A, B); CI count gate green (O); **render not read locally** | ✅ (gap noted) |
| R10 | in-process `s3s` endpoint, `#[ignore]` tier, `tests/s3-wire.sh`, CI step with a minimum of 46 | 46/46 locally (A, B, O) and on both CI legs (O); plain `cargo test` lists 42 ignored (A, B, O) | ✅ |

**Unmapped changes**, none blocking:

- The README's `-b 100000` becomes `-B 100000`.
- Four deferral seeds beyond the GOAL's three, each with a `ROADMAP.md` entry.
  `issues/s3-legacy-bucket-names.md` is new since cycle 2.
- `Cargo.lock` moves crates the S3 work does not name: `clap` goes from 4.5.58 to 4.6.7, `chrono`
  from 0.4.43 to 0.4.45 and `regex` from 1.12.3 to 1.13.1, with others (B, confirmed by O).
  `duckdb`, `arrow` and `parquet` are unchanged, so the GOAL's no-bump non-goal holds, and CI is
  green on the result.

### Findings

One finding, CONFIRMED. B raised it; A did not examine TLS. It is not an `invariants.md` §1–§12
violation, so it is not auto-CRITICAL.

#### [HIGH/CONFIRMED] 1. The S3 readers accept any TLS server certificate; the writer verifies
- **Where:** `src/engine.rs:171-178` (`open_s3_connection_from`). The function loads `httpfs` and
  runs the bound `CREATE SECRET`, and sets nothing about certificate verification. B reads
  httpfs's `enable_server_cert_verification` as off by default (DuckDB's documented default), and
  `strings` on the pinned extension shows the setting and `ca_cert_file` exist. Nothing in
  `src/`, `tests/`, `doc/`, `README.md`, `AGENTS.md`, `issues/` or `ROADMAP.md` mentions either
  setting.
  - The writer's client (`src/s3.rs:852-866`, reqwest over rustls with its default verification)
    does verify.
- **Failure scenario:**
  - A reader resolves to HTTPS by default: every AWS read, and any `https://` endpoint override.
  - Anyone on the network path who presents a certificate of their own completes the handshake.
    The reader then sends its signed requests, with the session token when one is set.
  - It returns whatever index the interceptor serves. That discloses every path, owner and size
    in the crawled tree, and lets the interceptor forge query results.
  - The writer refuses the same endpoint, so the two directions apply different trust to one
    resolved endpoint. That is the writer–reader coherence the GOAL names as the risk to design
    against.
  - No R-ID names TLS, so this is not claimed as a contract breach. It is graded HIGH as a real
    bug on the common path, per the rubric.
- **Evidence:** O re-ran B's probe with its own script. It used B's file-backed loopback S3
  stand-in, with a self-signed certificate whose subject and issuer are both `CN=127.0.0.1` and
  which is in no trust store. The server started and stopped inside the one call, and its port
  refused a connect afterwards.
  - Over plain HTTP, `xdu SRC -o s3://bkt/idx` exited 0, and `xdu-find -i s3://bkt/idx --count`
    printed `6`.
  - Over HTTPS with the self-signed certificate (`AWS_ENDPOINT_URL_S3=https://127.0.0.1:PORT`):
    - `xdu SRC -o s3://bkt/idx2` exited 1, "cannot reach the S3 endpoint while listing the index
      prefix …". The server logged 0 requests for `idx2`.
    - `xdu-find -i s3://bkt/idx --count` printed `6` and exited 0, with empty stderr.
    - `xdu-find -i s3://bkt/idx -u beta` printed the partition's three paths.
    - The server served those reads 13 requests over TLS: 4 LIST, 4 marker HEAD, 2 marker GET and
      3 chunk GET.
  - B's own runs matched this. A handshake-only TLS server logged `4 HANDSHAKE OK` for the reader
    and `4 TLS SSLError` for the writer. Through B's TLS stand-in the reader printed `137` and the
    writer exited 1.
  - The real AWS path is the same code, but it was not exercised, since this host has no network
    beyond loopback.
- **Touches:** the high-blast `src/engine.rs` ("the one connection open, the extension load and
  `open_s3_connection`"). It sits beside R5 (one resolver for both directions), R2 (the same
  answers a local copy gives) and the GOAL's Outcome ("on any host").
- **Why the fix is not a one-liner:**
  - Turning verification on is one fixed-text `SET` after the `LOAD`.
  - Which roots the reader then trusts is a design call. `httpfs` brings its own TLS stack and
    `ca_cert_file`, and the writer trusts the platform store. On a host whose roots are not where
    that stack looks (macOS keeps them in the keychain), or on an air-gapped site with a private
    CA, verification could fail every real read.
  - The loopback acceptance cannot show which, because its endpoint speaks plain HTTP.

**Considered and dropped** (A and B, merged):

- **The marker HEAD-then-GET race:** the local reader has the same stat-then-read window, so this
  is parity (A, B).
- **Reader error bodies leaking the token:** DuckDB's error text carries no body; 0 hits (A, B).
- **Proxy variables honoured despite the docs:** dead proxies saw 0 requests from either side (A,
  B).
- **The local `-u "it's"` parser error:** the recorded §5 exception for local paths, which the
  GOAL lists as a Non-goal (A, B).
- **Listing pagination past 1000 keys:** the prune and the glob both work (A).
- **Redaction mangling the text around a one-character probe secret:** cosmetic; nothing leaked
  (A).
- **A literal `~` in `AWS_CONFIG_FILE` or `AWS_SHARED_CREDENTIALS_FILE`:** expansion is not required
  (A, B).
- **httpfs filling `s3_*` settings from `AWS_*` at `LOAD`:** this makes the code comment saying
  the secret appears in "no `duckdb_settings()` row" untrue under environment credentials. But
  nothing prints settings, and the secret listing is redacted, so no output carries it (B).
- **A half key pair in the config file failing resolution when the credentials file holds a whole
  pair:** deliberate fail-closed design (B).
- **The `**` marker LIST cost, and tokio's DNS threads missing from §7's thread budget:**
  throughput is a non-goal, and the budget gap is a nit (A, B).
- **`view_query::root_files` swallowing errors:** old local behaviour, moved unchanged (A).
- **The S3 open-and-gate block duplicated in both readers:** each step is a library call, and the
  shape mirrors the local pattern (A, B).
- **The writer wording a TLS failure as "cannot reach":** message nuance only (B).
- **AWS China endpoints, and virtual-hosted TLS for dotted bucket names:** real-provider
  behaviour, outside the loopback acceptance (B).
- **`encode_chunk` and `target/wire-*.log` absent from `AGENTS.md`:** no gate is degraded (B).

### Human-gate triggers

- **Triggered by path.** Finding 1 is CONFIRMED and sits in `src/engine.rs`, which is on the
  authoritative list. It breaks none of §1, §2, §4 or §5 as written: no credential becomes SQL
  text, and the marker ordering holds.
- **Bounded loop exhausted.** This is cycle 3, the last in the bound, and the third without
  convergence. The skill stops here. It does not recommend `/xdu-build` without the human.
- **For the human: the finding needs a design decision before any fix.** Two routes:
  1. **Fix it in a fourth, human-approved build cycle.**
     - Readers turn verification on with fixed SQL text after the `LOAD`.
     - A GOAL amendment says which roots both directions trust: the platform store, a
       resolver-chosen CA file fed to both `httpfs`'s `ca_cert_file` and the writer, or both.
     - A wire test gives all three tools a self-signed TLS endpoint, and each must refuse it.
     - A test endpoint signed by a test CA, which the chosen source names, must be accepted.
     - `invariants.md` §5 gains the rule so the next review checks it.
     - The open risk is that a root source the loopback endpoint cannot prove may break real AWS
       reads on one of the release platforms.
  2. **Amend the GOAL to record it as a known limitation.** File a seed and a `ROADMAP.md` entry,
     and state on `xdu-find.1`, `xdu-view.1` and in the README that the readers do not verify
     the server's certificate.
  - **Recommendation: route 1.** The readers send signed requests to AWS over HTTPS by default,
    and they are the half of the pilot every host runs.
- **Also for the human:** the local man-page render is still unread (above).

### Optional completeness sub-pass (separate reviewer; may see TECH.md)

Not run (no `completeness` argument).
