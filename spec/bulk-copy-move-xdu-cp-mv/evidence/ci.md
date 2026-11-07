# CI evidence: bulk-copy-move-xdu-cp-mv

Head `769255e` (P1–P5 plus a merge of `main` at `0547790`), runs 42313370339 (Tests),
42313370288 (Release), 42313370268 (Publish Image): every job `success`; publish/push jobs
skipped on a branch. Quoted verbatim from `target/ci/run-<id>.log`. This closes P5's
cross-filesystem checkpoint (GOAL Clarification: green on both OSes by 2026-11-10) on
2026-11-06; P5 is not cut. Earlier heads: P1 `c5bb277` Tests 42301219563, P2 `2661b61` Tests
42303349693, P3 `7b1b014` Tests 42305444904, P4 `bfbbe22` Tests 42306819260, Release
42306819058, Image 42306818813, all green. P5 `3b41fe6` and `2ea74ca` got no runs: the PR
conflicted with `main` after #36, and GitHub creates no pull_request check suite then.

- `Validate build (linux/amd64)	UNKNOWN STEP	2026-11-06T23:59:12.1005053Z #22 15.94 extension-check: ok platform=linux_amd64 engine=v1.4.4`
- `Validate build (linux/arm64)	UNKNOWN STEP	2026-11-06T23:52:47.1166247Z #22 11.19 extension-check: ok platform=linux_arm64 engine=v1.4.4`
- `Test (macos-15)	XFS tests	2026-11-06T23:44:20.9797860Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.14s`
- `Test (macos-15)	XFS tests	2026-11-06T23:44:20.9886940Z xfs-wire: '' 3 of 3 passed with XDU_TEST_XFS_DIR=/Users/runner/work/_temp/xdu-xfs`
- `Test (macos-15)	S3 tests	2026-11-06T23:45:43.6948040Z s3-wire: '' 54 of 54 passed`
- `Test (ubuntu-24.04)	XFS tests	2026-11-06T23:44:02.3630521Z test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s`
- `Test (ubuntu-24.04)	XFS tests	2026-11-06T23:44:02.3639597Z xfs-wire: '' 3 of 3 passed with XDU_TEST_XFS_DIR=/dev/shm/xdu-xfs`
- `Test (ubuntu-24.04)	S3 tests	2026-11-06T23:44:34.3857406Z s3-wire: '' 54 of 54 passed`
- `Packaging (man + completions generate)	Assert critical literals survive into the published man-page text	2026-11-06T23:40:15.9358564Z OK: every asserted literal survived into the published man-page text`
- `Build macOS (x86_64-apple-darwin)	Check extensions load (cold HOME)	2026-11-06T23:47:59.8214610Z extension-check: ok platform=osx_amd64 engine=v1.4.4`
- `Build macOS (aarch64-apple-darwin)	Check extensions load (cold HOME)	2026-11-06T23:45:05.0483290Z extension-check: ok platform=osx_arm64 engine=v1.4.4`
- `Build (x86_64-unknown-linux-gnu)	Check extensions load (cold HOME)	2026-11-06T23:57:54.9077466Z extension-check: ok platform=linux_amd64 engine=v1.4.4`
- `Build (aarch64-unknown-linux-gnu)	Check extensions load (cold HOME)	2026-11-06T23:54:30.0641479Z extension-check: ok platform=linux_arm64 engine=v1.4.4`
