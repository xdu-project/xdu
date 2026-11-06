# P2 CI evidence: s3-index-target

Run B of P2 copied these lines from the CI logs of commit `7fd8aa4` (draft PR #33). Each one
is quoted verbatim with its own timestamp. It is a record, not a gate.

## What was tested

- **Tests workflow, run 42012803115.** It ran on `pull_request`, so it checked out GitHub's
  merge ref, not the head commit. That merge is `1c29a7f`, the head `7fd8aa4` merged into
  `main` at `3ed22d5`. `main` had not moved past `3ed22d5`, which is the merge base of
  `7fd8aa4` and `main`. So the merge's tree is the tree of `7fd8aa4`.
  - Job `Test (ubuntu-24.04)`, step `Run actions/checkout@9c091bb…`:
    `2026-11-05T10:41:36.2295285Z HEAD is now at 1c29a7f Merge 7fd8aa4fd804afa9d429deb5bbe427f8bea33e3f into 3ed22d54e9eb3cc8d411f564108efb277c3dbf92`
  - Job `Test (macos-15)`, same step:
    `2026-11-05T10:41:36.7190580Z HEAD is now at 1c29a7f Merge 7fd8aa4fd804afa9d429deb5bbe427f8bea33e3f into 3ed22d54e9eb3cc8d411f564108efb277c3dbf92`
  - Job `Packaging (man + completions generate)`, same step:
    `2026-11-05T10:41:36.2771054Z HEAD is now at 1c29a7f Merge 7fd8aa4fd804afa9d429deb5bbe427f8bea33e3f into 3ed22d54e9eb3cc8d411f564108efb277c3dbf92`
- **Release workflow, run 42012803137.** It ran because the commit touches `Cargo.toml` and
  `Cargo.lock`, and checked out the head commit itself.
  - Job `Build (x86_64-unknown-linux-gnu)`, checkout step:
    `2026-11-05T10:42:47.8578878Z HEAD is now at 7fd8aa4 [feature] Build s3-index-target P2: xdu-find queries S3 in place`
- **Publish Image workflow, run 42012803136.** Both validate builds checked out the same merge
  ref `1c29a7f`, and both are green. TECH does not require them for P2.

The run URLs are `https://github.com/xdu-project/xdu/actions/runs/<id>`.

## The S3 wire tests (tests run 42012803115, step "S3 tests")

The floor for this phase is N ≥ 19. The workflow sets its minimum at 22, the file's listed
count. On each leg, all 22 tests reported `ok`.

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T10:46:52.5281726Z s3-wire: '' 22 of 22 passed`
- Job `Test (macos-15)`:
  `2026-11-05T10:44:18.8621390Z s3-wire: '' 22 of 22 passed`

## The R3 ratio (tests run 42012803115, step "S3 tests")

TECH requires the line from the ubuntu leg. The cost test's negative control printed its line
too, and both legs measured the same bytes.

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T10:46:52.5250849Z s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD 10, GET 9`
  `2026-11-05T10:46:52.5276671Z s3-scan-bytes: received 15151190 of 16784047 stored bytes (90.271%), 29 requests: LIST 2, HEAD 10, GET 17`
- Job `Test (macos-15)`:
  `2026-11-05T10:44:18.8569500Z s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD 10, GET 9`
  `2026-11-05T10:44:18.8664920Z s3-scan-bytes: received 15151190 of 16784047 stored bytes (90.271%), 29 requests: LIST 2, HEAD 10, GET 17`

## Man-page literals (tests run 42012803115)

Job `Packaging (man + completions generate)`, step "Assert critical literals survive into the
published man-page text". The step includes the 16 new `xdu-find.1` checks. The log holds no
`CORRUPT RENDER`, `RENDER FAILED` or `RENDER EMPTY` line except where the step's own script
text is echoed.

- `2026-11-05T10:41:51.5951769Z OK: every asserted literal survived into the published man-page text`

## Extension check (release run 42012803137, step "Check extensions load (cold HOME)")

TECH does not require these for P2. They are recorded because the release legs ran.

- Job `Build (x86_64-unknown-linux-gnu)`:
  `2026-11-05T10:53:29.6395348Z extension-check: ok platform=linux_amd64 engine=v1.4.4`
- Job `Build (aarch64-unknown-linux-gnu)`:
  `2026-11-05T10:54:49.3550374Z extension-check: ok platform=linux_arm64 engine=v1.4.4`
- Job `Build macOS (aarch64-apple-darwin)`:
  `2026-11-05T10:57:33.5751190Z extension-check: ok platform=osx_arm64 engine=v1.4.4`
- Job `Build macOS (x86_64-apple-darwin)`:
  `2026-11-05T10:46:21.2110500Z extension-check: ok platform=osx_amd64 engine=v1.4.4`
