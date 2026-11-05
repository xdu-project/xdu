# P3 CI evidence: s3-index-target

Run B of P3 copied these lines from the CI logs of commit `e705e92` (draft PR #33). Each one
is quoted verbatim with its own timestamp. It is a record, not a gate.

## What was tested

- **Tests workflow, run 42020057385.** It ran on `pull_request`, so it checked out GitHub's
  merge ref, not the head commit. That merge is `e91ac68`, the head `e705e92` merged into
  `main` at `3ed22d5`. `main` had not moved past `3ed22d5`, which is the merge base of
  `e705e92` and `main`. So the merge's tree is the tree of `e705e92`.
  - Job `Test (ubuntu-24.04)`, step `Run actions/checkout@9c091bb…`:
    `2026-11-05T11:34:33.5268456Z HEAD is now at e91ac68 Merge e705e92a520ab063ee4fcd1f13e100d58989cf19 into 3ed22d54e9eb3cc8d411f564108efb277c3dbf92`
  - Job `Test (macos-15)`, same step:
    `2026-11-05T11:34:39.6324460Z HEAD is now at e91ac68 Merge e705e92a520ab063ee4fcd1f13e100d58989cf19 into 3ed22d54e9eb3cc8d411f564108efb277c3dbf92`
  - Job `Packaging (man + completions generate)`, same step:
    `2026-11-05T11:34:34.1649818Z HEAD is now at e91ac68 Merge e705e92a520ab063ee4fcd1f13e100d58989cf19 into 3ed22d54e9eb3cc8d411f564108efb277c3dbf92`
- **Release workflow, run 42020057315.** It ran because the commit touches `Cargo.toml` and
  `Cargo.lock`, and checked out the head commit itself.
  - Job `Build (x86_64-unknown-linux-gnu)`, checkout step:
    `2026-11-05T11:50:53.2932101Z HEAD is now at e705e92 [feature] Build s3-index-target P3: xdu -o s3:// writes into the bucket`
  - Job `Build (aarch64-unknown-linux-gnu)`, checkout step:
    `2026-11-05T11:50:55.2095679Z HEAD is now at e705e92 [feature] Build s3-index-target P3: xdu -o s3:// writes into the bucket`
- **Publish Image workflow, run 42020057317.** Both validate builds checked out the same merge
  ref `e91ac68`, and both are green. TECH does not require them for P3.
  - Job `Validate build (linux/amd64)`:
    `2026-11-05T11:54:20.5306883Z #22 9.878 extension-check: ok platform=linux_amd64 engine=v1.4.4`
  - Job `Validate build (linux/arm64)`:
    `2026-11-05T11:47:33.3705615Z #22 6.481 extension-check: ok platform=linux_arm64 engine=v1.4.4`

The run URLs are `https://github.com/xdu-project/xdu/actions/runs/<id>`.

## The S3 wire tests (tests run 42020057385, step "S3 tests")

The floor for this phase is N ≥ 29. The workflow sets its minimum at 36, the file's listed
count. On each leg, all 36 tests passed, the 14 `write_` tests among them.

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T11:59:19.5433504Z s3-wire: '' 36 of 36 passed`
- Job `Test (macos-15)`:
  `2026-11-05T11:55:29.7269420Z s3-wire: '' 36 of 36 passed`

The unsafe-name test reports whether its non-UTF-8 case ran. It ran on the Linux leg only,
because APFS refuses such a name:

- Job `Test (ubuntu-24.04)`:
  `2026-11-05T11:59:19.5405572Z write-unsafe-names: 5 case(s), non-UTF-8 case ran`
- Job `Test (macos-15)`:
  `2026-11-05T11:55:29.7266280Z write-unsafe-names: 4 case(s), non-UTF-8 case skipped: the filesystem refuses the name`

## Glibc floor (release run 42020057315, step "Assert glibc floor")

This is the first push with ring linked into a runtime binary under manylinux_2_28; before
it, ring was built there only as a build dependency of libduckdb-sys. On both Linux legs,
every binary needs at most `GLIBC_2.28`. The log's only `floor breach`, `runtime breach` and
`unexpected dynamic needs` text is the step's own script being echoed.

- Job `Build (x86_64-unknown-linux-gnu)`:
  `2026-11-05T12:08:12.4156729Z xdu needs max GLIBC_2.28`
  `2026-11-05T12:08:12.4501990Z xdu-find needs max GLIBC_2.28`
  `2026-11-05T12:08:12.4877289Z xdu-view needs max GLIBC_2.28`
  `2026-11-05T12:08:12.5253462Z xdu-rm needs max GLIBC_2.28`
- Job `Build (aarch64-unknown-linux-gnu)`:
  `2026-11-05T12:02:46.2171212Z xdu needs max GLIBC_2.28`
  `2026-11-05T12:02:46.2444768Z xdu-find needs max GLIBC_2.28`
  `2026-11-05T12:02:46.2752338Z xdu-view needs max GLIBC_2.28`
  `2026-11-05T12:02:46.3060760Z xdu-rm needs max GLIBC_2.28`

## Extension check (release run 42020057315, step "Check extensions load (cold HOME)")

- Job `Build (x86_64-unknown-linux-gnu)`:
  `2026-11-05T12:08:24.0600074Z extension-check: ok platform=linux_amd64 engine=v1.4.4`
- Job `Build (aarch64-unknown-linux-gnu)`:
  `2026-11-05T12:02:53.9379340Z extension-check: ok platform=linux_arm64 engine=v1.4.4`
- Job `Build macOS (aarch64-apple-darwin)`:
  `2026-11-05T12:06:47.9579190Z extension-check: ok platform=osx_arm64 engine=v1.4.4`
- Job `Build macOS (x86_64-apple-darwin)`:
  `2026-11-05T12:30:55.0083260Z extension-check: ok platform=osx_amd64 engine=v1.4.4`

## Release build time on macOS x86_64 (release run 42020057315)

Job `Build macOS (x86_64-apple-darwin)` found no cargo cache, so its release build (fat LTO,
one codegen unit) compiled the whole new dependency tree from cold. The step "Build release
binaries and the extension probe" ran 39 minutes; the job ran from 11:50:23 to 12:33:15, about
43 minutes, inside its 90-minute `timeout-minutes`. P2's release run 42012803137 built the
same leg from a restored cache in 3m 03s. A cold cache on this leg is now the slowest path
through the release workflow.

- Step "Cache cargo build":
  `2026-11-05T11:50:57.6870900Z No cache found.`
- Step "Build release binaries and the extension probe", first and last lines:
  `2026-11-05T11:50:57.7020930Z ##[group]Run cargo build --release --locked --target x86_64-apple-darwin --bins --example extension-probe`
  ``2026-11-05T12:30:28.6110090Z     Finished `release` profile [optimized] target(s) in 39m 29s``
- Step "Set up job", first line, and step "Complete job":
  `2026-11-05T11:50:23.2628680Z Current runner version: '2.338.2'`
  `2026-11-05T12:33:15.1489900Z Cleaning up orphan processes`

The `Finished` line carries ANSI colour codes in the raw log; they are dropped above.

## Man-page literals (tests run 42020057385)

Job `Packaging (man + completions generate)`, step "Assert critical literals survive into the
published man-page text". The step includes the 11 new `xdu.1` checks. The log holds no
`CORRUPT RENDER`, `RENDER FAILED` or `RENDER EMPTY` line except where the step's own script
text is echoed.

- `2026-11-05T11:34:57.8951839Z OK: every asserted literal survived into the published man-page text`
