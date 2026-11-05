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
