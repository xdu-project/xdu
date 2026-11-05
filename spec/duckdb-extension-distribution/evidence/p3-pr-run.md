# P3 run B: the draft pull request's release run

The draft pull request's `release.yaml` run built all four legs from the branch head. Each leg
fetched the pinned extension set, assembled its tarball, and ran `packaging/extensions/check.sh`
on that tarball under a cold `HOME`. `Publish release assets` was skipped, as it must be on a
pull request. The lines below are copied verbatim from each leg's "Check extensions load (cold
HOME)" step.

run: https://github.com/xdu-project/xdu/actions/runs/41974811737
head: ab0f7d98cbe7d09ecab804d4230d54acd5fa3598

x86_64-unknown-linux-gnu: extension-check: ok platform=linux_amd64 engine=v1.4.4
aarch64-unknown-linux-gnu: extension-check: ok platform=linux_arm64 engine=v1.4.4
aarch64-apple-darwin: extension-check: ok platform=osx_arm64 engine=v1.4.4
x86_64-apple-darwin: extension-check: ok platform=osx_amd64 engine=v1.4.4

## Notes from the logs

- **Checkout.** Every job, `Prepare` and the four build legs, checked out
  `ab0f7d98cbe7d09ecab804d4230d54acd5fa3598` (`HEAD is now at ab0f7d9`), the branch head rather
  than a merge ref.
- **glibc floor.** Both Linux legs ran "Assert glibc floor" before the fetch and passed: `xdu`,
  `xdu-find`, `xdu-view` and `xdu-rm` each need at most `GLIBC_2.28`, with no `GLIBCXX`/`GCC`
  ceiling breach and no unexpected dynamic dependency.
- **The two-C++-runtimes risk on `linux_amd64`.** The `httpfs` boundary query returned httpfs's
  own error, with no abort:
  `boundary=ok IO Error: Could not establish connection error for HTTP HEAD to 'http://127.0.0.1:9/x.parquet'`.
  The other three legs printed the same line. No `.cargo/config.toml` change is needed, so no
  deferral is filed for it.
- **Other checks on the pull request.** Tests (Lint, Test on `ubuntu-24.04` and `macos-15`,
  Packaging) passed. The Publish Image run's two "Validate build" jobs passed, and its push jobs
  were skipped.
