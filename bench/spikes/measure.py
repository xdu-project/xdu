#!/usr/bin/env python3
"""Drives one query per process through probes/query.rs and attributes the proxy's
request log to it. Prints the tables in research/duckdb-native-s3-reads.md.

Each query runs in a fresh process against a fresh DuckDB connection, and the proxy log
is truncated immediately before. A shared connection would carry DuckDB's own buffer
cache between queries and make whichever ran second look free.

Usage:  python3 measure.py read | hive | cluster
Env:    EP (default 127.0.0.1:19001, i.e. the proxy), PROXY_LOG, REPO
"""

import json
import os
import subprocess
import sys

LOG = os.environ.get("PROXY_LOG", "/tmp/xdu-proxy.log")
EP = os.environ.get("EP", "127.0.0.1:19001")
REPO = os.environ.get("REPO", os.getcwd())

BIG = "s3://xdu-probe/big/*/*.parquet"
TWO = "s3://xdu-hive/two/*/*/*.parquet"
CUT = 1600000000 + 5000 * 300  # oldest ~5% of the cluster fixture by atime

SETS = {
    # Read cost against the 80.1 MiB index: what a query fetches vs. what staging would.
    "read": [
        ("count(*) all partitions", f"SELECT count(*) FROM read_parquet('{BIG}')"),
        ("count(*) one partition", "SELECT count(*) FROM read_parquet('s3://xdu-probe/big/alice/*.parquet')"),
        ("sum(size) all", f"SELECT sum(size) FROM read_parquet('{BIG}')"),
        ("selective: size > 29.99M", f"SELECT count(*) FROM read_parquet('{BIG}') WHERE size > 29999990"),
        ("top 10 by size", f"SELECT count(*) FROM (SELECT path FROM read_parquet('{BIG}') ORDER BY size DESC LIMIT 10)"),
        ("path LIKE (full scan)", f"SELECT count(*) FROM read_parquet('{BIG}') WHERE path LIKE '%file999999'"),
        ("LIMIT 5, no order", f"SELECT count(*) FROM (SELECT path FROM read_parquet('{BIG}') LIMIT 5)"),
    ],
    # What a hive predicate prunes, and what it does not (LIST is flat either way).
    "hive": [
        ("no predicate (all 120)", f"SELECT count(*) FROM read_parquet('{TWO}', hive_partitioning=true)"),
        ("WHERE partition='p42'", f"SELECT count(*) FROM read_parquet('{TWO}', hive_partitioning=true) WHERE partition='p42'"),
        ("WHERE ayear='2022' (24/120)", f"SELECT count(*) FROM read_parquet('{TWO}', hive_partitioning=true) WHERE ayear='2022'"),
        ("partition IN (3 of 120)", f"SELECT count(*) FROM read_parquet('{TWO}', hive_partitioning=true) WHERE partition IN ('p1','p2','p3')"),
        ("glob-scoped to one prefix", "SELECT count(*) FROM read_parquet('s3://xdu-hive/two/partition=p42/*/*.parquet')"),
        ("mixed layout, WHERE ayear", "SELECT count(*) FROM read_parquet('s3://xdu-hive/mixed/*/*/*.parquet', hive_partitioning=true) WHERE ayear='2022'"),
    ],
    # Row-group sizing x sort order, against an age predicate. Both halves or neither.
    "cluster": [
        (f"{label}: {q}", sql)
        for label, variant in [
            ("today (walk, 1rg)", "c_walk_1rg"),
            ("sorted, 1rg", "c_sort_1rg"),
            ("walk, 10rg", "c_walk_10rg"),
            ("sorted, 10rg", "c_sort_10rg"),
        ]
        for q, sql in [
            ("oldest 5% count", f"SELECT count(*) FROM read_parquet('s3://xdu-hive/{variant}/*/*.parquet') WHERE atime < {CUT}"),
            ("oldest 5% sum", f"SELECT sum(size) FROM read_parquet('s3://xdu-hive/{variant}/*/*.parquet') WHERE atime < {CUT}"),
        ]
    ],
}


def account():
    """Fold the proxy log into (LIST, GET, ranged, bytes)."""
    lists = gets = ranged = 0
    total = 0
    try:
        lines = open(LOG).read().splitlines()
    except FileNotFoundError:
        return 0, 0, 0, 0
    for line in lines:
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if entry["m"] != "GET":
            continue
        if "list-type" in entry.get("q", ""):
            lists += 1
        else:
            gets += 1
            if entry.get("range"):
                ranged += 1
        total += entry.get("bytes", 0)
    return lists, gets, ranged, total


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else ""
    if which not in SETS:
        sys.exit(f"usage: measure.py {' | '.join(SETS)}")

    print(f"{'query':34} {'result':>15} {'ms':>6} {'LIST':>5} {'GET':>5} {'ranged':>7} {'MiB':>8}")
    for label, sql in SETS[which]:
        open(LOG, "w").close()
        env = dict(os.environ, SQL=sql, EP=EP)
        out = subprocess.run(
            ["cargo", "test", "--test", "query", "--", "--nocapture"],
            capture_output=True, text=True, env=env, cwd=REPO,
        ).stdout
        result = ms = "?"
        for line in out.splitlines():
            if line.startswith("RESULT="):
                result = line.split("RESULT=")[1].split(" MS=")[0]
                ms = line.split("MS=")[1]
        lists, gets, ranged, total = account()
        print(f"{label:34} {result:>15} {ms:>6} {lists:>5} {gets:>5} {ranged:>7} {total/1048576:>8.2f}")


if __name__ == "__main__":
    main()
