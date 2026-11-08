# P2 CI evidence: hive-partition-columns-find-rm

Run B of P2 copied these lines from the CI logs of commit `5ef684b`
(`5ef684bf6d9d0e83ab1c979c4da60f0046cdb74d`, draft PR #38). Each one is quoted verbatim with
its own timestamp, from `target/ci/run-<id>.log`. It is a record, not a gate. It closes P2's
*Left for CI* item: the S3 wire tier, which this host cannot run, is the only execution of the
two wire tests (GOAL R5 accepts CI evidence for the `s3://` arm).

The pull request's check rollup on that head reported `mergeStateStatus` CLEAN, with every job
that ran `success` and `Publish release assets` skipped. These two facts come from the rollup, not from the logs. Neither saved log holds
a `##[error]` line.

## What was tested

- **Tests workflow, run 42526947270.** It ran on `pull_request` and checked out the merge ref.
  That merge is `bf7327e`, the head `5ef684b` merged into `main` at `052bbc7`. `052bbc7` is
  `5ef684b`'s merge base and an ancestor of it, so the merge's tree is `5ef684b`'s.
  - Job `Lint (fmt + clippy)`, step `Run actions/checkout@9c091bb…`:
    `2026-11-08T21:47:36.6996005Z HEAD is now at bf7327e Merge 5ef684bf6d9d0e83ab1c979c4da60f0046cdb74d into 052bbc75528fa0a80d6eaf468e9520d906375388`
  - Job `Test (ubuntu-24.04)`, same step:
    `2026-11-08T21:47:37.6475803Z HEAD is now at bf7327e Merge 5ef684bf6d9d0e83ab1c979c4da60f0046cdb74d into 052bbc75528fa0a80d6eaf468e9520d906375388`
  - Job `Test (macos-15)`, same step:
    `2026-11-08T21:47:39.0204690Z HEAD is now at bf7327e Merge 5ef684bf6d9d0e83ab1c979c4da60f0046cdb74d into 052bbc75528fa0a80d6eaf468e9520d906375388`
  - Job `Packaging (man + completions generate)`, same step:
    `2026-11-08T21:47:36.6316199Z HEAD is now at bf7327e Merge 5ef684bf6d9d0e83ab1c979c4da60f0046cdb74d into 052bbc75528fa0a80d6eaf468e9520d906375388`
- **Release workflow, run 42526947295.** P2's diff (with P1's) touches `src/lib.rs`, one of
  `release.yaml`'s `pull_request` paths, so it ran. It checked out the head commit itself, with
  `TAG: pr38`. Job `Prepare (man pages + completions)` and the four build legs ran. The saved
  log holds no lines for `Publish release assets`, whose
  `if: github.event_name != 'pull_request'` skips it on a pull request.
  - Job `Prepare (man pages + completions)`, checkout step:
    `2026-11-08T21:47:38.0018916Z HEAD is now at 5ef684b [fix] Build hive-partition-columns-find-rm P2: S3 wire tests and docs`
  - Job `Build (aarch64-unknown-linux-gnu)`, checkout step:
    `2026-11-08T22:03:30.0834905Z HEAD is now at 5ef684b [fix] Build hive-partition-columns-find-rm P2: S3 wire tests and docs`
  - Job `Build (x86_64-unknown-linux-gnu)`, checkout step:
    `2026-11-08T22:03:31.0322169Z HEAD is now at 5ef684b [fix] Build hive-partition-columns-find-rm P2: S3 wire tests and docs`
  - Job `Build macOS (aarch64-apple-darwin)`, checkout step:
    `2026-11-08T22:03:02.7377390Z HEAD is now at 5ef684b [fix] Build hive-partition-columns-find-rm P2: S3 wire tests and docs`
  - Job `Build macOS (x86_64-apple-darwin)`, checkout step:
    `2026-11-08T22:03:07.7330270Z HEAD is now at 5ef684b [fix] Build hive-partition-columns-find-rm P2: S3 wire tests and docs`
- **Publish Image workflow: no run.** No path in the pull request's diff matches
  `docker.yaml`'s `pull_request` `paths` filter (`Dockerfile`, `.dockerignore`, the workflow
  itself, `Cargo.toml`, `Cargo.lock`, `examples/**`, `packaging/extensions/**`), so GitHub
  created no run for it. TECH does not require it for P2.

The run URLs are `https://github.com/xdu-project/xdu/actions/runs/<id>`.

## Tests run 42526947270

### The S3 wire tests (step "S3 tests")

The workflow's minimum is 56, the file's listed count, and both legs passed all 56, the two new
tests among them.

- Job `Test (ubuntu-24.04)`:
  - `2026-11-08T21:54:07.0021668Z ##[group]Run sh tests/s3-wire.sh "" 56`
  - `2026-11-08T21:54:55.6045963Z test find_key_value_names_read_as_bare_names ... ok`
  - `2026-11-08T21:54:55.6126697Z test view_key_value_names_read_as_bare_names ... ok`
  - `2026-11-08T21:54:55.6128885Z test result: ok. 56 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 48.08s`
  - `2026-11-08T21:54:55.6131063Z s3-wire: '' 56 of 56 passed`
- Job `Test (macos-15)`:
  - `2026-11-08T21:55:01.8401420Z ##[group]Run sh tests/s3-wire.sh "" 56`
  - `2026-11-08T21:56:19.6068530Z test find_key_value_names_read_as_bare_names ... ok`
  - `2026-11-08T21:56:19.6127310Z test view_key_value_names_read_as_bare_names ... ok`
  - `2026-11-08T21:56:19.6128650Z test result: ok. 56 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 76.54s`
  - `2026-11-08T21:56:19.6129750Z s3-wire: '' 56 of 56 passed`

In the plain `Test` step, before the wire tier, both new tests print `ignored, needs the pinned
httpfs; …` on both legs (ubuntu at `21:52:20.5109683Z` and `21:52:20.5269156Z`, macOS at
`21:52:44.7083470Z` and `21:52:45.7203440Z`), as their siblings do. Their red on `main` is by
construction (PLAN §2.4), not observed.

### The six local tests (step "Test")

P1's six tests also ran on both operating systems; this host ran them on macOS only.

- Job `Test (ubuntu-24.04)`:
  - `2026-11-08T21:52:08.5418578Z test find_reads_a_key_value_partition_as_a_bare_name ... ok`
  - `2026-11-08T21:52:14.9312835Z test find_reads_an_index_under_a_key_value_directory_as_a_bare_one ... ok`
  - `2026-11-08T21:52:17.5581436Z test rm_reads_a_key_value_partition_as_a_bare_name ... ok`
  - `2026-11-08T21:52:20.5030374Z test rm_reads_an_index_under_a_key_value_directory_as_a_bare_one ... ok`
  - `2026-11-08T21:52:28.6859384Z test view_reads_a_key_value_partition_as_a_bare_name ... ok`
  - `2026-11-08T21:54:02.5453367Z test view_reads_an_index_under_a_key_value_directory_as_a_bare_one ... ok`
- Job `Test (macos-15)`:
  - `2026-11-08T21:50:43.6275910Z test find_reads_a_key_value_partition_as_a_bare_name ... ok`
  - `2026-11-08T21:51:45.5888600Z test find_reads_an_index_under_a_key_value_directory_as_a_bare_one ... ok`
  - `2026-11-08T21:52:05.9008960Z test rm_reads_a_key_value_partition_as_a_bare_name ... ok`
  - `2026-11-08T21:52:44.2649080Z test rm_reads_an_index_under_a_key_value_directory_as_a_bare_one ... ok`
  - `2026-11-08T21:52:59.4441090Z test view_reads_a_key_value_partition_as_a_bare_name ... ok`
  - `2026-11-08T21:54:54.2172810Z test view_reads_an_index_under_a_key_value_directory_as_a_bare_one ... ok`

### Other steps

- Job `Test (ubuntu-24.04)`, step `XFS tests`:
  `2026-11-08T21:54:03.9701019Z xfs-wire: '' 3 of 3 passed with XDU_TEST_XFS_DIR=/dev/shm/xdu-xfs`
- Job `Test (macos-15)`, step `XFS tests`:
  `2026-11-08T21:54:57.7786810Z xfs-wire: '' 3 of 3 passed with XDU_TEST_XFS_DIR=/Users/runner/work/_temp/xdu-xfs`
- Job `Lint (fmt + clippy)`, step `Clippy` ran
  `cargo clippy --all-targets --all-features -- -D warnings`, printed no warning, and ended on
  cargo's `Finished` line for the `dev` profile at 2026-11-08T21:48:29.0008950Z (that line
  carries ANSI colour codes, so it is described rather than quoted). Step `Format check` ran
  `cargo fmt --all -- --check` and printed nothing.
- Job `Packaging (man + completions generate)`:
  - step `Assert critical literals survive into the published man-page text`:
    `2026-11-08T21:48:01.7391071Z OK: every asserted literal survived into the published man-page text`
  - step `Assert container specs match the HPCCM recipe`:
    `2026-11-08T21:48:21.5141934Z OK: container specs match the recipe`
  - step `Assert all man pages + completions were produced`:
    `2026-11-08T21:48:21.5214133Z OK: all six man pages and all bash+zsh completions present`

## Release run 42526947295

### glibc floor (step "Assert glibc floor")

- Job `Build (aarch64-unknown-linux-gnu)`:
  - `2026-11-08T22:15:04.7552282Z xdu needs max GLIBC_2.28`
  - `2026-11-08T22:15:04.7836366Z xdu-find needs max GLIBC_2.28`
  - `2026-11-08T22:15:04.8130195Z xdu-view needs max GLIBC_2.28`
  - `2026-11-08T22:15:04.8429528Z xdu-rm needs max GLIBC_2.28`
  - `2026-11-08T22:15:04.8727357Z xdu-cp needs max GLIBC_2.28`
  - `2026-11-08T22:15:04.9031505Z xdu-mv needs max GLIBC_2.28`
- Job `Build (x86_64-unknown-linux-gnu)`:
  - `2026-11-08T22:22:20.0979490Z xdu needs max GLIBC_2.28`
  - `2026-11-08T22:22:20.1365692Z xdu-find needs max GLIBC_2.28`
  - `2026-11-08T22:22:20.1799378Z xdu-view needs max GLIBC_2.28`
  - `2026-11-08T22:22:20.2228245Z xdu-rm needs max GLIBC_2.28`
  - `2026-11-08T22:22:20.2653269Z xdu-cp needs max GLIBC_2.28`
  - `2026-11-08T22:22:20.3097172Z xdu-mv needs max GLIBC_2.28`

### Extensions load from each tarball (step "Check extensions load (cold HOME)")

- Job `Build (aarch64-unknown-linux-gnu)`:
  `2026-11-08T22:15:17.5728989Z extension-check: ok platform=linux_arm64 engine=v1.4.4`
- Job `Build (x86_64-unknown-linux-gnu)`:
  `2026-11-08T22:22:40.1637576Z extension-check: ok platform=linux_amd64 engine=v1.4.4`
- Job `Build macOS (aarch64-apple-darwin)`:
  `2026-11-08T22:15:22.3660920Z extension-check: ok platform=osx_arm64 engine=v1.4.4`
- Job `Build macOS (x86_64-apple-darwin)`:
  `2026-11-08T22:26:27.4432540Z extension-check: ok platform=osx_amd64 engine=v1.4.4`
