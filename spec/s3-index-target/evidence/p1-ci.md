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
