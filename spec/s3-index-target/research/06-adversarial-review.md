# Research 06 — Adversarial review of the test gates

R3, R7 and R10 rest on test infrastructure: a byte meter, an opt-in test tier, and a CI guard. A
design error there fails green, so one reviewer was asked for inputs the drafted gates pass while
the feature is broken, or fail while it is good. It read GOAL, PLAN, the digest, brief 04,
`src/engine.rs` and the CI workflow. All 13 findings were accepted. The fixes live in `TECH.md`
(Conventions and the phase checklists) and in [`verify/wire.sh`](../verify/wire.sh).

| # | Severity | Hole in the draft | Disposition |
|---|---|---|---|
| 1 | high | Under `--include-ignored` libtest always prints `0 ignored`, so the regex checked only "≥1 passed". An early-returning test reports `ok` | `verify/wire.sh`: passed count must equal the `--list` count for the prefix, at or above a per-phase minimum. Early `return` in `tests/s3_tests.rs` is refused. `TestS3::start()` always panics when it cannot bind or find httpfs |
| 2 | medium | `tee \| grep -q` without pipefail ignores cargo's status, and with pipefail a SIGPIPE turns it red at random | Cargo's exit status is read directly, with the log written to a file first. CI uses the same rule |
| 3 | high | R3 could pass with zero chunk GETs, and a `--count` answered from the marker's `files=` would pass | The set of range-fetched keys must equal the set of chunk keys. Status, `Content-Range`, and open-ended ranges count as whole |
| 4 | high | Only `--count` was metered, and reader-side disk was never checked (a persistent secret, a staged range) | Every wire test goes through the proxy with no whole-object GET. Every child's `HOME`, `TMPDIR` and cwd start and stay empty |
| 5 | medium | The denominator counted the marker and any sidecar files. The fixture floor was not enforced, and the fixture shape was not xdu's | Sum only `*.parquet` under the prefix. Assert ≥10 MiB, ≥4 partitions and ≤12 chunks at run time. Encode with xdu's own encoder |
| 6 | low–med | A thin false-red margin at 12 chunks, and a vague negative control | About 8 chunks of about 2 MiB, ratio printed. The negative control is the binary with a non-matching `-p`, required to exceed 25%. Strip `DUCKDB_*`. Config names only the proxy |
| 7 | medium | A proxy that parses only the first request on a kept-alive connection misses later GETs | Force `Connection: close` and assert requests = connections, or parse every request |
| 8 | high | Writer R4 cases fail at the DELETE too, so an early marker clear goes unseen | Add rejections with valid credentials and assert no DELETE was seen. Assert request order LIST → DELETE → PUT chunks → PUT marker |
| 9 | high | The round trip compared only `xdu-find`'s default paths, so mangled columns or a `COPY` re-encode would pass | Compare all eight columns per partition in-test, plus Parquet schema, KV metadata and `created_by` |
| 10 | medium | The local-disk check missed cwd and the source tree. Nothing showed chunks go up as they finalize, and the stale re-run might have had one chunk | Empty cwd, source listing unchanged, at least one PUT before an injected failure, and assert more than one chunk before the re-run |
| 11 | medium | The cap test passed on a wrong key (404). One message for every R4 case would pass. A released port may be reused | stderr names the cap, a HEAD of the exact key is seen, and the boundary pair is tested. A distinct cause per case. Dead endpoint `127.0.0.1:1` |
| 12 | high | The garbage-extension test passed with a discarded load error, covered only `xdu-find`, and asserted a substring the default path already contains | Assert empty stderr. Cover `xdu-rm` and local `xdu`. Grep refuses discarded `load_extension` results. Assert "release tarball". Nothing at the default lookup. An empty directory, not an empty value |
| 13 | medium | Several verify greps were satisfiable without the work: `otool` absent on Linux, a comment matching `XDU_EXTENSION_DIR`, an empty issue file, a P1 CI step unchecked, and no behavioural R6 test | `cargo tree -i` for `openssl-sys`/`native-tls`/`aws-lc-sys`. Literal greps scoped to the page's `check` block. `test -s` plus `## Problem`. P1 greps the CI step. R6 wire test with `'` in the prefix |
