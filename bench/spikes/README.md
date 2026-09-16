# Spike probes

**These are not the benchmark.** The harness beside them — `gen_tree.py`, `run.sh`,
`scenarios.md`, `HPC-PROTOCOL.md`, `results/baseline.json` — is a consistent suite for detecting
crawl performance regressions over time, with comparability rules and a committed reference. What is
in this directory is the opposite kind of artifact: throwaway instruments, written to answer one
question once, kept only because the answers became commitments.

They live here rather than in `tests/` because Cargo compiles everything under `tests/`, and these
need a MinIO container, reach the network, and several of them deliberately print a failure instead
of asserting one. `cargo test` must not depend on any of that. Cargo ignores `bench/` entirely — the
directory is `bench/`, not `benches/`, and no `[[bench]]` target is declared — so the probes sit
here inert until `run.sh` copies one into `tests/` for the length of a single run.

## What they establish

Every claim and both cost tables in
[`research/duckdb-native-s3-reads.md`](../../research/duckdb-native-s3-reads.md) come from these.
The mapping is the point of keeping them: a number in that brief should be traceable to the code
that produced it.

| probe | establishes | needs |
|---|---|---|
| `extension.rs` | claims 1-5 — the bundled engine loads prebuilt `httpfs`; install redirects off `$HOME`; a pre-seeded directory serves a cold `HOME` with no network; a vendored binary loads by path; an absent one fails naming the path | network once, for claim 1 |
| `layout.rs` | the layout is not Hive-partitioned; the `filename` regexp `xdu-view` relies on; the four repack traps in [`issues/xdu-repack.md`](../../issues/xdu-repack.md) | nothing — all local |
| `fixtures.rs` | claim 12 — `COPY ... TO 's3://…'` writes the layout — and builds what the tables measure | MinIO |
| `query.rs` | nothing on its own; runs one query per process so the proxy log is attributable | MinIO + proxy |
| `capability.rs` | claims 7, 10, 11 — marker readable over S3; `read_parquet(?)` takes a bound parameter; credentials via the config API never touch SQL — plus both open items | MinIO + `FIXTURE=big` |

Claim 6 (a real `xdu` index answering the readers' SQL over `s3://`) is not a probe: build an index
with the real binary and upload it, as under *Running* below. Claim 13 (the Linux `httpfs` glibc
floor) was read off the ELF and is the one claim no probe covers.

## Running

```sh
docker run -d --rm --name xdu-spike-minio -p 19000:9000 \
  -e MINIO_ROOT_USER=probekey -e MINIO_ROOT_PASSWORD=probesecret \
  quay.io/minio/minio:latest server /data

export AWS_ACCESS_KEY_ID=probekey AWS_SECRET_ACCESS_KEY=probesecret AWS_DEFAULT_REGION=us-east-1
aws --endpoint-url http://127.0.0.1:19000 s3 mb s3://xdu-probe
aws --endpoint-url http://127.0.0.1:19000 s3 mb s3://xdu-hive

sh bench/spikes/run.sh layout                      # local only, no endpoint
sh bench/spikes/run.sh extension
FIXTURE=big sh bench/spikes/run.sh fixtures        # also hive, cluster
sh bench/spikes/run.sh capability

# Claim 6: a real index, not a synthetic one.
./target/release/xdu /some/tree -o /tmp/idx
aws --endpoint-url http://127.0.0.1:19000 s3 cp --recursive /tmp/idx s3://xdu-probe/indexes/scratch/

# The cost tables. The proxy must be running; measure.py points query.rs at it.
python3 bench/spikes/proxy.py &
REPO=$PWD python3 bench/spikes/measure.py read     # also hive, cluster
```

`FIXTURE=cluster` takes the longest — forty objects of a hundred thousand rows each.

## Two things that will bite

**Write fixtures direct to 19000, measure reads through 19001.** The proxy exists to attribute
reads; routing writes through it logs a great deal of noise and slows the build considerably.
`fixtures.rs` defaults to the endpoint and `query.rs` to the proxy for that reason.

**One query per process is load-bearing.** DuckDB caches buffers on a connection, so a second query
against the same objects reads far less than the first and the cheap ones look free. `measure.py`
spawns a fresh process per query and truncates the proxy log immediately before it. Any
consolidation of that loop invalidates the tables.

## When this directory goes away

It is tied to one cycle. When `s3-index-target` and the layout seeds have landed, whatever is still
worth running should have become a real test or a real benchmark scenario, and the rest should be
deleted along with `research/duckdb-native-s3-reads.md`. A spike harness kept past the decision it
informed is just code nobody runs.
