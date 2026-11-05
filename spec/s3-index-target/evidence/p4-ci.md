# P4 CI evidence: s3-index-target

Run B of P4 copied these lines from the CI logs of commit `ebe5a48` (draft PR #33). Each one
is quoted verbatim with its own timestamp. It is a record, not a gate.

## What was tested

- **Tests workflow, run 42034745391.** It ran on `pull_request`, so it checked out GitHub's
  merge ref, not the head commit. That merge is `b005caf`, the head `ebe5a48` merged into
  `main` at `a0cdd95`. Unlike P1 to P3, `main` had moved past the merge base `3ed22d5`: PR #32
  (`a0cdd95`) retired a seed and edited `ROADMAP.md` and six files under `issues/`, and touched
  nothing else. So the merge's tree is the tree of `ebe5a48` plus those documentation changes;
  every source, test, workflow and man-page file is as in `ebe5a48`.
  - Job `Test (ubuntu-24.04)`, step `Run actions/checkout@9c091bb…`:
    `2026-11-05T13:19:31.5350998Z HEAD is now at b005caf Merge ebe5a48e6bd2f4ed1e0dbd2fe521d1278036b1cd into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
  - Job `Test (macos-15)`, same step:
    `2026-11-05T13:19:33.9484260Z HEAD is now at b005caf Merge ebe5a48e6bd2f4ed1e0dbd2fe521d1278036b1cd into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
  - Job `Packaging (man + completions generate)`, same step:
    `2026-11-05T13:19:32.3342199Z HEAD is now at b005caf Merge ebe5a48e6bd2f4ed1e0dbd2fe521d1278036b1cd into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
- **Release workflow, run 42034745344.** It checked out the head commit itself. TECH does not
  require it for P4; it is recorded because it ran green.
  - Job `Build (x86_64-unknown-linux-gnu)`, checkout step:
    `2026-11-05T13:35:31.3289033Z HEAD is now at ebe5a48 [feature] Build s3-index-target P4: xdu-view browses S3 in place`
  - Job `Build (aarch64-unknown-linux-gnu)`, checkout step:
    `2026-11-05T13:35:31.8180662Z HEAD is now at ebe5a48 [feature] Build s3-index-target P4: xdu-view browses S3 in place`
- **Publish Image workflow, run 42034745180.** Both validate builds checked out the same merge
  ref `b005caf`, and both are green. TECH does not require them for P4.
  - Job `Validate build (linux/amd64)`:
    `2026-11-05T13:37:56.6985905Z #24 11.07 extension-check: ok platform=linux_amd64 engine=v1.4.4`
  - Job `Validate build (linux/arm64)`:
    `2026-11-05T13:32:09.3709214Z #24 6.547 extension-check: ok platform=linux_arm64 engine=v1.4.4`

The run URLs are `https://github.com/xdu-project/xdu/actions/runs/<id>`.

## The S3 wire tests (tests run 42034745391, step "S3 tests")

The floor for this phase is N ≥ 34. The workflow sets its minimum at 41, the file's listed
count. On each leg, all 41 tests passed, the 5 `view_` tests among them.

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T13:44:15.4661069Z s3-wire: '' 41 of 41 passed`
- Job `Test (macos-15)`:
  `2026-11-05T13:38:15.2246680Z s3-wire: '' 41 of 41 passed`

The `view_` tests, by name:

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T13:44:15.4644916Z test view_failures_name_distinct_causes_before_the_terminal ... ok`
  `2026-11-05T13:44:15.4648069Z test view_partition_list_matches_local_for_every_sort ... ok`
  `2026-11-05T13:44:15.4649755Z test view_quotes_in_the_prefix_and_a_directory_match_local ... ok`
  `2026-11-05T13:44:15.4659372Z test view_directories_match_local_for_every_sort ... ok`
  `2026-11-05T13:44:15.4659848Z test view_filtered_views_match_local ... ok`
- Job `Test (macos-15)`:
  `2026-11-05T13:38:15.2216010Z test view_failures_name_distinct_causes_before_the_terminal ... ok`
  `2026-11-05T13:38:15.2217450Z test view_directories_match_local_for_every_sort ... ok`
  `2026-11-05T13:38:15.2218600Z test view_partition_list_matches_local_for_every_sort ... ok`
  `2026-11-05T13:38:15.2221210Z test view_quotes_in_the_prefix_and_a_directory_match_local ... ok`
  `2026-11-05T13:38:15.2245640Z test view_filtered_views_match_local ... ok`

The R3 measure, unchanged from P2:

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T13:44:15.4634821Z s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD 10, GET 9`
- Job `Test (macos-15)`:
  `2026-11-05T13:38:15.2187340Z s3-count-bytes: received 138351 of 16784047 stored bytes (0.824%), 21 requests: LIST 2, HEAD 10, GET 9`

## Man-page literals (tests run 42034745391)

Job `Packaging (man + completions generate)`, step "Assert critical literals survive into the
published man-page text". The step includes the 16 `xdu-view.1` checks this phase added (its
script line, echoed in the log, begins `check share/man/man1/xdu-view.1 '2x:XDU_INDEX'
'2x:XDU_EXTENSION_DIR'`). The log holds no `CORRUPT RENDER`, `RENDER FAILED` or `RENDER EMPTY`
line except where the step's own script text is echoed.

- `2026-11-05T13:20:17.9523110Z OK: every asserted literal survived into the published man-page text`

## Release legs (release run 42034745344)

Not required for P4. Step "Assert glibc floor": every binary needs at most `GLIBC_2.28` on both
Linux legs; the log's only `floor breach`, `runtime breach` and `unexpected dynamic needs` text
is the step's own script being echoed. Step "Check extensions load (cold HOME)": all four legs
ok.

- Job `Build (x86_64-unknown-linux-gnu)`:
  `2026-11-05T13:49:08.3855914Z xdu needs max GLIBC_2.28`
  `2026-11-05T13:49:08.4100518Z xdu-find needs max GLIBC_2.28`
  `2026-11-05T13:49:08.4360148Z xdu-view needs max GLIBC_2.28`
  `2026-11-05T13:49:08.4629672Z xdu-rm needs max GLIBC_2.28`
  `2026-11-05T13:49:18.6404099Z extension-check: ok platform=linux_amd64 engine=v1.4.4`
- Job `Build (aarch64-unknown-linux-gnu)`:
  `2026-11-05T13:47:21.4390353Z xdu needs max GLIBC_2.28`
  `2026-11-05T13:47:21.4679328Z xdu-find needs max GLIBC_2.28`
  `2026-11-05T13:47:21.4988276Z xdu-view needs max GLIBC_2.28`
  `2026-11-05T13:47:21.5292109Z xdu-rm needs max GLIBC_2.28`
  `2026-11-05T13:47:29.3549979Z extension-check: ok platform=linux_arm64 engine=v1.4.4`
- Job `Build macOS (aarch64-apple-darwin)`:
  `2026-11-05T13:52:16.5169760Z extension-check: ok platform=osx_arm64 engine=v1.4.4`
- Job `Build macOS (x86_64-apple-darwin)`:
  `2026-11-05T13:42:59.3923000Z extension-check: ok platform=osx_amd64 engine=v1.4.4`
