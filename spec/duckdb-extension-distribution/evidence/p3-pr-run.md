# P3 run B: the draft pull request's release run

The draft pull request's `release.yaml` run built all four legs from the branch head. Each leg
fetched the pinned extension set, assembled its tarball, and ran `packaging/extensions/check.sh`
on that tarball under a cold `HOME`. `Publish release assets` was skipped, as it must be on a
pull request. The lines below are copied verbatim from each leg's "Check extensions load (cold
HOME)" step.

This run refreshes the evidence after review cycle 1. The earlier run, at ab0f7d9, predates the
remediation's changes to `check.sh`, the probe and `Cargo.toml`, so it no longer described the
release path.

run: https://github.com/xdu-project/xdu/actions/runs/41983483238
head: 78daf41eba2d95cbf0c4333cec7aa48ba3f95d6a

x86_64-unknown-linux-gnu: extension-check: ok platform=linux_amd64 engine=v1.4.4
aarch64-unknown-linux-gnu: extension-check: ok platform=linux_arm64 engine=v1.4.4
aarch64-apple-darwin: extension-check: ok platform=osx_arm64 engine=v1.4.4
x86_64-apple-darwin: extension-check: ok platform=osx_amd64 engine=v1.4.4

## Notes from the logs

- **Checkout.** Every job, `Prepare` and the four build legs, checked out
  `78daf41eba2d95cbf0c4333cec7aa48ba3f95d6a` (`HEAD is now at 78daf41`), the branch head rather
  than a merge ref. `TAG` was `pr31`.
- **Every shipped binary executed (review F1).** At 78daf41, `check.sh` runs `--version` for
  `xdu`, `xdu-find`, `xdu-view` and `xdu-rm` and keeps the output, so the log shows no version
  lines. The order of the output shows the loop ran. On every leg, `xdu`'s `INFO Indexing` lines
  follow `loaded=`/`boundary=`. The `--version` loop sits between those two points under `set -eu`.
  No leg printed a `check.sh:` failure, and each step ends on its `extension-check: ok` line.
- **glibc floor.** Both Linux legs ran "Assert glibc floor" before the fetch and passed. `xdu`,
  `xdu-find`, `xdu-view` and `xdu-rm` each need at most `GLIBC_2.28`.
- **The two-C++-runtimes risk on `linux_amd64`.** The `httpfs` boundary query returned httpfs's
  own error, with no abort:
  `boundary=ok IO Error: Could not establish connection error for HTTP HEAD to 'http://127.0.0.1:9/x.parquet'`.
  The other three legs printed the same line.
- **Other checks on the pull request at 78daf41.** These were reported with the run, not read from
  this log. Tests (Lint, Test on `ubuntu-24.04` and `macos-15`, Packaging) passed, and both of the
  Publish Image run's "Validate build" jobs passed.
