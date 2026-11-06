# P6 CI evidence: s3-index-target

Run B of P6 copied these lines from the CI logs of commit `5711e9d` (draft PR #33). Each one
is quoted verbatim with its own timestamp. It is a record, not a gate.

## What was tested

- **Tests workflow, run 42145425932.** It ran on `pull_request` and checked out the merge ref.
  That merge is `9f88f7e`, the head `5711e9d` merged into `main` at `a0cdd95`. `main` has not
  moved past `a0cdd95`, the merge base, so the merge's tree is `5711e9d`'s.
  - Job `Test (ubuntu-24.04)`, step `Run actions/checkout@9c091bb…`:
    `2026-11-06T01:40:52.1544580Z HEAD is now at 9f88f7e Merge 5711e9d2b5bb071163f63265b4c04e8dfe767124 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
  - Job `Test (macos-15)`, same step:
    `2026-11-06T01:40:53.0161040Z HEAD is now at 9f88f7e Merge 5711e9d2b5bb071163f63265b4c04e8dfe767124 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
  - Job `Packaging (man + completions generate)`, same step:
    `2026-11-06T01:40:50.8363350Z HEAD is now at 9f88f7e Merge 5711e9d2b5bb071163f63265b4c04e8dfe767124 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
  - Job `Lint (fmt + clippy)`, same step:
    `2026-11-06T01:40:51.0549697Z HEAD is now at 9f88f7e Merge 5711e9d2b5bb071163f63265b4c04e8dfe767124 into a0cdd95201e6cf2dff44f68ab3c6362dae907143`
- **Release workflow, run 42145425938.** P6's diff touches `src/engine.rs` and `Cargo.toml`, so
  it ran on the pull request. It checked out the head commit itself. Job `Prepare (man pages +
  completions)` and the four build legs ran. The saved log holds no lines for `Publish release
  assets`, whose `if: github.event_name != 'pull_request'` skips it on a pull request.
  - Job `Build (aarch64-unknown-linux-gnu)`, checkout step:
    `2026-11-06T01:42:28.4906917Z HEAD is now at 5711e9d [feature] Build s3-index-target P6: TLS server authentication for both directions`
  - Job `Build (x86_64-unknown-linux-gnu)`, checkout step:
    `2026-11-06T01:42:28.8676887Z HEAD is now at 5711e9d [feature] Build s3-index-target P6: TLS server authentication for both directions`
  - Job `Build macOS (aarch64-apple-darwin)`, checkout step:
    `2026-11-06T01:42:02.4961780Z HEAD is now at 5711e9d [feature] Build s3-index-target P6: TLS server authentication for both directions`
  - Job `Build macOS (x86_64-apple-darwin)`, checkout step:
    `2026-11-06T01:42:01.4091410Z HEAD is now at 5711e9d [feature] Build s3-index-target P6: TLS server authentication for both directions`
- **Publish Image workflow, run 42145425951.** Both validate builds checked out the merge ref
  `9f88f7e`. TECH does not require them for P6; their probe lines are quoted below because they
  ran.
- No log of the three runs holds a `##[error]` line.

The run URLs are `https://github.com/xdu-project/xdu/actions/runs/<id>`.

## What the AWS probe proves

Every probe line below is an HTTP 403 from real AWS (`InvalidAccessKeyId`, or "refused the
credentials ... (HTTP 403)"), answering AWS's documented example key that
`packaging/extensions/aws-tls-probe.sh` sends. No line names a certificate, `AWS_CA_BUNDLE` or
"cannot reach". A 403 means the TLS handshake to AWS completed in each tool, so each host's
platform bundle was found and AWS's real chain verified against it. As the GOAL clarification
for P6 rules, that is a platform smoke check: it cannot show that verification is on, since AWS
answers 403 with it off too. The loopback `tls_` wire tests own that, and remain the whole
acceptance.

The release containers (manylinux_2_28, RHEL-family) resolved the RHEL path
`/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem`; the Ubuntu leg and the image builds
resolved `/etc/ssl/certs/ca-certificates.crt`; every macOS host resolved `/etc/ssl/cert.pem`.

## Tests run 42145425932

### The S3 wire tests (step "S3 tests")

The workflow's minimum is 51, the file's listed count, and both legs passed all 51, the five
`tls_` tests among them.

- Job `Test (ubuntu-24.04)`:
  `2026-11-06T01:53:30.4428303Z s3-wire: '' 51 of 51 passed`
  `2026-11-06T01:53:30.4417415Z test tls_a_self_signed_endpoint_is_refused_by_all_three_tools ... ok`
  `2026-11-06T01:53:30.4417723Z test tls_only_the_resolved_file_is_trusted ... ok`
- Job `Test (macos-15)`:
  `2026-11-06T01:47:10.3228800Z s3-wire: '' 51 of 51 passed`
  `2026-11-06T01:47:10.3219230Z test tls_a_self_signed_endpoint_is_refused_by_all_three_tools ... ok`
  `2026-11-06T01:47:10.3219570Z test tls_only_the_resolved_file_is_trusted ... ok`

### The red-first's Linux writer half (test 4(a))

**Not shown by these logs.** The plan asks run B to confirm, on Linux, that wire test 4(a)
fails on the writer as it stood before P6 (whose platform verifier honours `SSL_CERT_FILE` on
Linux). No CI run executed test 4 against that tree: the test first exists in `5711e9d`, and
the Ubuntu leg ran it only against the fixed `src/`. The line above,
`test tls_only_the_resolved_file_is_trusted ... ok` on `Test (ubuntu-24.04)`, shows that on
Linux all three tools, the writer among them, now refuse an endpoint trusted only through
`SSL_CERT_FILE` with "TLS certificate is not trusted". That is the green half on Linux, not the
red one. No Linux host was available to this run B to replay the old tree.

### AWS TLS probe (step "AWS TLS probe", after "S3 tests")

- Job `Test (ubuntu-24.04)`:
  `2026-11-06T01:53:30.4543054Z aws-tls-probe: platform bundle /etc/ssl/certs/ca-certificates.crt`
  `2026-11-06T01:53:31.9440083Z aws-tls-probe: xdu-find: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:53:33.3636499Z aws-tls-probe: xdu-view: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:53:33.5203608Z aws-tls-probe: xdu: 2026-11-06T01:53:33Z ERROR the S3 endpoint refused the credentials while listing the index prefix for s3://xdu-tls-probe/p; check AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or the profile AWS_PROFILE names, and that they may write there (Generic S3 error: Error performing list request: Error performing GET https://xdu-tls-probe.s3.us-east-1.amazonaws.com/?list-type=2&prefix=p%2F in 138.294406ms - HTTP 403 InvalidAccessKeyId)`
- Job `Test (macos-15)`:
  `2026-11-06T01:47:10.3751030Z aws-tls-probe: platform bundle /etc/ssl/cert.pem`
  `2026-11-06T01:47:12.1490650Z aws-tls-probe: xdu-find: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:47:13.7893080Z aws-tls-probe: xdu-view: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:47:13.9966430Z aws-tls-probe: xdu: 2026-11-06T01:47:13Z ERROR the S3 endpoint refused the credentials while listing the index prefix for s3://xdu-tls-probe/p; check AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or the profile AWS_PROFILE names, and that they may write there (Generic S3 error: Error performing list request: Error performing GET https://xdu-tls-probe.s3.us-east-1.amazonaws.com/?list-type=2&prefix=p%2F in 128.424667ms - HTTP 403 InvalidAccessKeyId)`

### Man-page literals and lint

- Job `Packaging (man + completions generate)`, step "Assert critical literals survive into the
  published man-page text", whose checks now carry `AWS_CA_BUNDLE` for `xdu.1`, `xdu-find.1`
  and `xdu-view.1`:
  `2026-11-06T01:41:18.2633828Z OK: every asserted literal survived into the published man-page text`
- Job `Lint (fmt + clippy)` checked out the same merge (above), ran its "Format check" and
  "Clippy" steps to "Complete job", and logged no `##[error]`. Clippy's last build line, at
  `2026-11-06T01:41:40.5876084Z`, is cargo's `Finished` for the `dev` profile (colour codes
  left out here).

## Release run 42145425938 (step "Check extensions load (cold HOME)")

`check.sh` runs the probe with the cold `HOME`, while the extracted `lib/` is still in place,
then prints `extension-check: ok`.

- Job `Build (aarch64-unknown-linux-gnu)`:
  `2026-11-06T01:54:50.4125335Z aws-tls-probe: platform bundle /etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem`
  `2026-11-06T01:54:51.7177100Z aws-tls-probe: xdu-find: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:54:52.9642340Z aws-tls-probe: xdu-view: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:54:53.0929937Z aws-tls-probe: xdu: 2026-11-06T01:54:53Z ERROR the S3 endpoint refused the credentials while listing the index prefix for s3://xdu-tls-probe/p; check AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or the profile AWS_PROFILE names, and that they may write there (Generic S3 error: Error performing list request: Error performing GET https://xdu-tls-probe.s3.us-east-1.amazonaws.com/?list-type=2&prefix=p%2F in 118.464037ms - HTTP 403 InvalidAccessKeyId)`
  `2026-11-06T01:54:53.1504108Z extension-check: ok platform=linux_arm64 engine=v1.4.4`
- Job `Build (x86_64-unknown-linux-gnu)`:
  `2026-11-06T01:56:07.6809681Z aws-tls-probe: platform bundle /etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem`
  `2026-11-06T01:56:08.8816832Z aws-tls-probe: xdu-find: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:56:10.1201467Z aws-tls-probe: xdu-view: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:56:10.2561209Z aws-tls-probe: xdu: 2026-11-06T01:56:10Z ERROR the S3 endpoint refused the credentials while listing the index prefix for s3://xdu-tls-probe/p; check AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or the profile AWS_PROFILE names, and that they may write there (Generic S3 error: Error performing list request: Error performing GET https://xdu-tls-probe.s3.us-east-1.amazonaws.com/?list-type=2&prefix=p%2F in 126.486688ms - HTTP 403 InvalidAccessKeyId)`
  `2026-11-06T01:56:10.3070519Z extension-check: ok platform=linux_amd64 engine=v1.4.4`
- Job `Build macOS (aarch64-apple-darwin)`:
  `2026-11-06T01:56:36.5854250Z aws-tls-probe: platform bundle /etc/ssl/cert.pem`
  `2026-11-06T01:56:37.6680350Z aws-tls-probe: xdu-find: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:56:38.5051960Z aws-tls-probe: xdu-view: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:56:38.5667620Z aws-tls-probe: xdu: 2026-11-06T01:56:38Z ERROR the S3 endpoint refused the credentials while listing the index prefix for s3://xdu-tls-probe/p; check AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or the profile AWS_PROFILE names, and that they may write there (Generic S3 error: Error performing list request: Error performing GET https://xdu-tls-probe.s3.us-east-1.amazonaws.com/?list-type=2&prefix=p%2F in 21.264375ms - HTTP 403 InvalidAccessKeyId)`
  `2026-11-06T01:56:38.7170650Z extension-check: ok platform=osx_arm64 engine=v1.4.4`
- Job `Build macOS (x86_64-apple-darwin)`:
  `2026-11-06T01:48:09.7568290Z aws-tls-probe: platform bundle /etc/ssl/cert.pem`
  `2026-11-06T01:48:11.8512130Z aws-tls-probe: xdu-find: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:48:13.9026690Z aws-tls-probe: xdu-view: Error: the S3 endpoint refused the credentials while listing the index for s3://xdu-tls-probe/p (HTTP 403); check the access key, secret and region`
  `2026-11-06T01:48:14.1538630Z aws-tls-probe: xdu: 2026-11-06T01:48:14Z ERROR the S3 endpoint refused the credentials while listing the index prefix for s3://xdu-tls-probe/p; check AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or the profile AWS_PROFILE names, and that they may write there (Generic S3 error: Error performing list request: Error performing GET https://xdu-tls-probe.s3.us-east-1.amazonaws.com/?list-type=2&prefix=p%2F in 213.303529ms - HTTP 403 InvalidAccessKeyId)`
  `2026-11-06T01:48:14.6314570Z extension-check: ok platform=osx_amd64 engine=v1.4.4`

## Publish Image run 42145425951 (not required for P6)

The image builds run `check.sh` too, inside the image build.

- Job `Validate build (linux/arm64)`:
  `2026-11-06T01:53:03.2349406Z #22 6.431 aws-tls-probe: platform bundle /etc/ssl/certs/ca-certificates.crt`
  `2026-11-06T01:53:05.6176102Z #22 8.836 extension-check: ok platform=linux_arm64 engine=v1.4.4`
- Job `Validate build (linux/amd64)`:
  `2026-11-06T02:00:04.6586902Z #22 9.719 aws-tls-probe: platform bundle /etc/ssl/certs/ca-certificates.crt`
  `2026-11-06T02:00:06.3050446Z #22 11.52 extension-check: ok platform=linux_amd64 engine=v1.4.4`

Each image leg's three tool lines are 403 answers of the same form as above.
