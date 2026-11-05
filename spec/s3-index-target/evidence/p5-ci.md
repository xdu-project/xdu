# P5 CI evidence: s3-index-target

Run B of P5 copied these lines from the CI logs of commit `b6673d3` (draft PR #33). Each one
is quoted verbatim with its own timestamp. It is a record, not a gate.

## What was tested

- **Tests workflow, run 42040878700.** It ran on `pull_request`, so it checked out GitHub's
  merge ref, not the head commit. That merge is `506097e`, the head `b6673d3` merged into
  `main` at `a0cdd95`. The branch already contains `a0cdd95` (merged in `a8cda44`), so `a0cdd95`
  is the merge base and the merge's tree is the tree of `b6673d3`.
  - Job `Test (ubuntu-24.04)`, step `Run actions/checkout@9c091bb…`:
    `2026-11-05T14:00:55.1742762Z HEAD is now at 506097e Merge b6673d34dfb51f39d6812cf583497a5a1d975a15 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
  - Job `Test (macos-15)`, same step:
    `2026-11-05T14:00:47.7689660Z HEAD is now at 506097e Merge b6673d34dfb51f39d6812cf583497a5a1d975a15 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
  - Job `Packaging (man + completions generate)`, same step:
    `2026-11-05T14:00:45.3039628Z HEAD is now at 506097e Merge b6673d34dfb51f39d6812cf583497a5a1d975a15 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
- **Release workflow, run 42040878683** and **Publish Image workflow, run 42040878670.** TECH
  does not require either for P5. The release log saved for this run B is empty, and the image
  log saved holds the `linux/arm64` validate build's `extension-check` line only, so neither
  workflow's results are quoted here beyond that one line:
  - Job `Validate build (linux/arm64)`:
    `2026-11-05T14:14:18.6808832Z #24 6.577 extension-check: ok platform=linux_arm64 engine=v1.4.4`

The run URLs are `https://github.com/xdu-project/xdu/actions/runs/<id>`.

## The S3 wire tests (tests run 42040878700, step "S3 tests")

The floor for this phase is N ≥ 34, and the workflow's minimum must equal the listed count. It is
41, and on each leg all 41 tests passed.

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T14:04:57.2020487Z s3-wire: '' 41 of 41 passed`
- Job `Test (macos-15)`:
  `2026-11-05T14:04:28.6545530Z s3-wire: '' 41 of 41 passed`

The R3 measure and the unsafe-name case, unchanged from P3 and P4:

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T14:04:57.1922963Z s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD 10, GET 9`
  `2026-11-05T14:04:57.2005843Z write-unsafe-names: 5 case(s), non-UTF-8 case ran`
- Job `Test (macos-15)`:
  `2026-11-05T14:04:28.6491820Z s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD 10, GET 9`
  `2026-11-05T14:04:28.6516700Z write-unsafe-names: 4 case(s), non-UTF-8 case skipped: the filesystem refuses the name`

## Man-page literals (tests run 42040878700)

Job `Packaging (man + completions generate)`, step "Assert critical literals survive into the
published man-page text", with the checks for all four pages (`xdu.1`, `xdu-find.1`,
`xdu-view.1`, `xdu-rm.1`). The log holds no `CORRUPT RENDER`, `RENDER FAILED` or `RENDER EMPTY`
line except where the step's own script text is echoed.

- `2026-11-05T14:01:14.5201116Z OK: every asserted literal survived into the published man-page text`

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

### Man-page literals and lint (tests run 42058060960)

- Job `Packaging (man + completions generate)`, step "Assert critical literals survive into the
  published man-page text":
  `2026-11-05T15:45:14.2401794Z OK: every asserted literal survived into the published man-page text`
- Job `Lint (fmt + clippy)` checked out the same merge (`2026-11-05T15:44:46.8193770Z HEAD is now at 0d80d6a …`) and logged no `##[error]`.
