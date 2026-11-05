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
