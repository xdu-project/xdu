# P1 CI evidence: s3-index-target

Run B of P1 copied these lines from the CI logs of commit `e6135fd` (draft PR #33). Each one
is quoted verbatim with its own timestamp. It is a record, not a gate.

## What was tested

- **Tests workflow, run 42003486290.** It ran on `pull_request`, so it checked out GitHub's
  merge ref `refs/remotes/pull/33/merge`, not the head commit. That merge is `2c60a95`, the head
  `e6135fd` merged into `main` at `3ed22d5`. `main` had not moved past `3ed22d5`, which is the
  merge base of `e6135fd` and `main`. So the merge's tree is the tree of `e6135fd`.
  - Job `Test (macos-15)`, step `Run actions/checkout@9c091bb…`:
    `2026-11-05T09:34:35.8387960Z HEAD is now at 2c60a95 Merge e6135fd071c0ef617be4ec3c8ce98d8120c59155 into 3ed22d54e9eb3cc8d411f564108efb277c3dbf92`
  - Job `Test (ubuntu-24.04)`, same step:
    `2026-11-05T09:34:33.5024074Z HEAD is now at 2c60a95 Merge e6135fd071c0ef617be4ec3c8ce98d8120c59155 into 3ed22d54e9eb3cc8d411f564108efb277c3dbf92`
- **Release workflow, run 42003486169.** It checked out the head commit itself.
  - Job `Build (x86_64-unknown-linux-gnu)`, checkout step:
    `2026-11-05T09:47:27.3918029Z HEAD is now at e6135fd [feature] Build s3-index-target P1: resolver, S3 connection, harness`
  - Job `Build (aarch64-unknown-linux-gnu)`, checkout step:
    `2026-11-05T09:47:27.7618448Z HEAD is now at e6135fd [feature] Build s3-index-target P1: resolver, S3 connection, harness`

## Fetching the pinned httpfs (tests run 42003486290)

The step "Fetch the pinned httpfs for the S3 tests" is green on both legs. Each leg's
`footer.sh` check printed `ok`. The tests log holds no `##[error]` line, and on each leg the
next step, "S3 tests", ran. GitHub skips that step after a failure.

- Job `Test (macos-15)`:
  `2026-11-05T09:51:28.5389290Z footer: ok /Users/runner/work/_temp/xdu-ext/httpfs.duckdb_extension engine=v1.4.4 platform=osx_arm64 extension_version=13f8a81 abi=CPP sha256=9a3eef9c7779b381bd02d2743eb96585e25e9e4e6a0ffb8f4f06ff91e9afe8be`
- Job `Test (ubuntu-24.04)`:
  `2026-11-05T09:55:18.1473764Z footer: ok /home/runner/work/_temp/xdu-ext/httpfs.duckdb_extension engine=v1.4.4 platform=linux_amd64 extension_version=13f8a81 abi=CPP sha256=ff726450b5374caaf9ffd9ef8fd9ec40df19b514117f079de3611326432a6646`

The release workflow has no such step: its legs fetch httpfs in their own packaging steps.

## The S3 wire tests (tests run 42003486290, step "S3 tests")

The floor for this phase is N ≥ 5, and the workflow sets its minimum at 7. On each leg, all
five `harness_` tests and both always-run tests reported `ok`. The ubuntu line is the first
httpfs load by a debug build on linux_amd64.

- Job `Test (macos-15)`:
  `2026-11-05T09:51:34.6993110Z s3-wire: '' 7 of 7 passed`
- Job `Test (ubuntu-24.04)`:
  `2026-11-05T09:55:20.0053101Z s3-wire: '' 7 of 7 passed`

## Extension check (release run 42003486169, step "Check extensions load (cold HOME)")

- Job `Build (x86_64-unknown-linux-gnu)`:
  `2026-11-05T10:04:51.8579593Z extension-check: ok platform=linux_amd64 engine=v1.4.4`
- Job `Build (aarch64-unknown-linux-gnu)`:
  `2026-11-05T09:58:48.7097592Z extension-check: ok platform=linux_arm64 engine=v1.4.4`
- The macOS legs passed too, though TECH does not require them:
  - `Build macOS (aarch64-apple-darwin)`:
    `2026-11-05T10:01:46.1294220Z extension-check: ok platform=osx_arm64 engine=v1.4.4`
  - `Build macOS (x86_64-apple-darwin)`:
    `2026-11-05T10:12:27.9248640Z extension-check: ok platform=osx_amd64 engine=v1.4.4`

## Review cycle 1 (remediation run B, head `fbecdb7`)

Run B of the review-cycle-1 remediation copied these lines verbatim from the CI logs of
`fbecdb7` on draft PR #33. A record, not a gate.

- **Tests workflow, run 42058060960.** It ran on `pull_request` and checked out the merge ref
  `pull/33/merge`. That merge is `0d80d6a`, the head `fbecdb7` merged into `main` at `a0cdd95`.
  `main` has not moved past `a0cdd95`, the merge base, so the merge's tree is `fbecdb7`'s.
  - Job `Test (ubuntu-24.04)`, step `Run actions/checkout@9c091bb…`:
    `2026-11-05T15:44:44.6867098Z HEAD is now at 0d80d6a Merge fbecdb7255e4f97bf43669ba953329e91f1e4793 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
  - Job `Test (macos-15)`, same step:
    `2026-11-05T15:44:49.1877470Z HEAD is now at 0d80d6a Merge fbecdb7255e4f97bf43669ba953329e91f1e4793 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
- No log of run 42058060960, release run 42058061145 or image run 42058061014 holds a
  `##[error]` line.

### The S3 wire tests (tests run 42058060960, step "S3 tests")

The workflow's minimum is 43, the file's listed count. Both legs passed all of them, the new
`harness_stray_duckdb_s3_endpoint_is_not_taken_when_the_resolver_picks_aws` and
`write_refusal_bodies_that_echo_the_session_token_are_not_printed` among them.

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T15:48:49.0594620Z s3-wire: '' 43 of 43 passed`
  `2026-11-05T15:48:49.0321873Z s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD 10, GET 9`
- Job `Test (macos-15)`:
  `2026-11-05T15:48:18.9658130Z s3-wire: '' 43 of 43 passed`
  `2026-11-05T15:48:18.9687020Z s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD 10, GET 9`

### Release run 42058061145 (checks out `fbecdb7` itself)

The run happened although the remediation diff touches none of the release workflow's
pull-request paths: GitHub filters a `pull_request` run by the pull request's whole diff against
`main`, which still includes `Cargo.toml` and `src/engine.rs` from earlier phases. So the release
legs built the new explicit object_store endpoint.

- Checkout, job `Build (x86_64-unknown-linux-gnu)`:
  `2026-11-05T15:46:01.6826165Z HEAD is now at fbecdb7 [feature] Build s3-index-target P1/P3/P5: review cycle 1 F1-F6`
- Checkout, job `Build (aarch64-unknown-linux-gnu)`:
  `2026-11-05T15:46:09.8936447Z HEAD is now at fbecdb7 [feature] Build s3-index-target P1/P3/P5: review cycle 1 F1-F6`
- Step "Assert glibc floor". The log's only `floor breach`, `runtime breach` and
  `unexpected dynamic needs` text is the step's own script being echoed.
  - Job `Build (x86_64-unknown-linux-gnu)`:
    `2026-11-05T16:02:32.5748745Z xdu needs max GLIBC_2.28`
    `2026-11-05T16:02:32.6033758Z xdu-find needs max GLIBC_2.28`
    `2026-11-05T16:02:32.6342824Z xdu-view needs max GLIBC_2.28`
    `2026-11-05T16:02:32.6652712Z xdu-rm needs max GLIBC_2.28`
  - Job `Build (aarch64-unknown-linux-gnu)`:
    `2026-11-05T15:58:28.0715781Z xdu needs max GLIBC_2.28`
    `2026-11-05T15:58:28.1009951Z xdu-find needs max GLIBC_2.28`
    `2026-11-05T15:58:28.1336082Z xdu-view needs max GLIBC_2.28`
    `2026-11-05T15:58:28.1662728Z xdu-rm needs max GLIBC_2.28`
- Step "Check extensions load (cold HOME)":
  - Job `Build (x86_64-unknown-linux-gnu)`:
    `2026-11-05T16:03:06.5573387Z extension-check: ok platform=linux_amd64 engine=v1.4.4`
  - Job `Build (aarch64-unknown-linux-gnu)`:
    `2026-11-05T15:58:36.2219821Z extension-check: ok platform=linux_arm64 engine=v1.4.4`
  - Job `Build macOS (aarch64-apple-darwin)`:
    `2026-11-05T15:54:16.9685420Z extension-check: ok platform=osx_arm64 engine=v1.4.4`
  - Job `Build macOS (x86_64-apple-darwin)`:
    `2026-11-05T15:59:08.0791240Z extension-check: ok platform=osx_amd64 engine=v1.4.4`
