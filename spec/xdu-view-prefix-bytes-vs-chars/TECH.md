---
slug: xdu-view-prefix-bytes-vs-chars
title: xdu-view lists full child names under a non-ASCII directory
kind: fix
appetite: small
status: in_progress
branch: fix/xdu-view-prefix-bytes-vs-chars
base: main
current_phase: P2
last_updated: '2026-11-06'
phases:
- id: P1
  name: Count the prefix in code points; prove it locally, red on 7689a92
  status: done
  satisfies:
  - R1
  - R2
  - R3
  - R4
  - R5
  - R6
  - R7
  depends_on: []
  parallel: false
  hammerable: false
  hill: downhill
  verify: 'set -eu; mkdir -p target; cargo test --locked --lib view_query:: >target/p1-lib.log
    2>&1; for t in local_view_sql_is_byte_for_byte_what_xdu_view_sent s3_view_sql_binds_the_glob_and_the_directory_prefix
    directory_offset_counts_code_points; do grep -qF "test view_query::tests::$t ...
    ok" target/p1-lib.log; done; cargo test --locked --test view_tests >target/p1-view.log
    2>&1; grep -qF "test non_ascii_directories_list_full_names_in_both_views ... ok"
    target/p1-view.log; grep -qF "hôme-ü" tests/view_tests.rs; sh spec/xdu-view-prefix-bytes-vs-chars/verify/scope.sh;
    for tree in base mutant-utf16 mutant-nomarks mutant-nofffd; do sh spec/xdu-view-prefix-bytes-vs-chars/verify/red.sh
    "$tree" "local list full names under" non_ascii_directories_list_full_names_in_both_views
    --test view_tests non_ascii_directories_list_full_names_in_both_views -- --exact;
    done; cargo test --locked; cargo clippy --locked --all-targets --all-features
    -- -D warnings; cargo fmt --all -- --check; echo P1-GATE-GREEN'
- id: P2
  name: 'The S3 arm: wire test with explicit names; wire count 54; ledger'
  status: pending
  satisfies:
  - R4
  - R5
  depends_on:
  - P1
  parallel: false
  hammerable: false
  hill: uphill
  verify: 'set -eu; mkdir -p target; if [ ! -f target/test-ext/httpfs.duckdb_extension
    ]; then cargo build --locked --example extension-probe; sh packaging/extensions/fetch.sh
    target/debug/examples/extension-probe target/test-ext; fi; XDU_TEST_EXTENSION_DIR=target/test-ext
    sh tests/s3-wire.sh view_non_ascii 1; grep -qF "test view_non_ascii_directories_list_full_names
    ... ok" target/wire-view_non_ascii.log; grep -B1 "^fn view_non_ascii_directories_list_full_names()"
    tests/s3_tests.rs | grep -qF ''#[ignore = "needs the pinned httpfs''; XDU_TEST_EXTENSION_DIR=target/test-ext
    sh tests/s3-wire.sh view_ 6; listed=$(cargo test --locked --test s3_tests -- --list
    --include-ignored 2>/dev/null | grep -c ": test$"); test "$listed" -eq 54; grep
    -qF ''sh tests/s3-wire.sh "" 54'' .github/workflows/test.yaml; grep -qF ''XDU_TEST_EXTENSION_DIR=target/test-ext
    sh tests/s3-wire.sh "" 54'' AGENTS.md; if grep -nF ''s3-wire.sh "" 53'' .github/workflows/test.yaml
    AGENTS.md; then echo "a stale wire count remains" >&2; exit 1; fi; XDU_TEST_EXTENSION_DIR=target/test-ext
    sh tests/s3-wire.sh "" 54; for tree in base mutant-utf16 mutant-nomarks mutant-nofffd;
    do sh spec/xdu-view-prefix-bytes-vs-chars/verify/red.sh "$tree" "s3 list full
    names under" view_non_ascii_directories_list_full_names --test s3_tests view_non_ascii_directories_list_full_names
    -- --include-ignored --exact; done; sh spec/xdu-view-prefix-bytes-vs-chars/verify/scope.sh;
    test -s issues/xdu-view-like-metacharacters.md; grep -qF "## Problem" issues/xdu-view-like-metacharacters.md;
    grep -qF "issues/xdu-view-like-metacharacters.md" ROADMAP.md; cargo test --locked;
    cargo clippy --locked --all-targets --all-features -- -D warnings; cargo fmt --all
    -- --check; echo P2-GATE-GREEN'
review:
  last_reviewed_commit: ''
  verdict: none
  blocked_reason: ''
  cycle: 0
---
# TECH.md — `xdu-view` lists full child names under a non-ASCII directory

The **context engine and finite-state machine** for this fix. The YAML frontmatter above is the
resume ground-truth (read it with
`uv run --with pyyaml python .agents/factory/bin/next_phase.py spec/xdu-view-prefix-bytes-vs-chars/TECH.md`);
the per-phase checklists below are the work. `xdu-build` executes the next actionable phase, runs
its `verify:` command, updates state via
`uv run --with pyyaml python .agents/factory/bin/set_phase.py …`, and makes one atomic code+state
commit.

- **Vision / requirements (locked):** [`GOAL.md`](GOAL.md) — R-IDs are the contract.
- **Authoritative design:** [`PLAN.md`](PLAN.md). Lean path: no `research/`; the DuckDB 1.4.4
  evidence and the `LIKE` verdict are in PLAN §4.

## Frontmatter field reference

As in [the template](../../.agents/factory/templates/TECH.md): `status`, `appetite`, phase
`status`, `satisfies`, `depends_on`, `parallel`, `hammerable`, `hill`, `attempts`, `verify`,
`review.cycle`.

## Conventions (apply to every phase)

- Commit conventions, code style and load-bearing invariants come from
  [`AGENTS.md`](../../AGENTS.md); the curated checklist is
  [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) (§5 is the one this fix
  touches; `src/view_query.rs` is on the high-blast list).
- One phase per `xdu-build` invocation; one atomic commit with the code and this file's state.
  Subjects `[fix] Build xdu-view-prefix-bytes-vs-chars P<n>: …`, 72 characters at most. No
  `Co-Authored-By` trailer.
- No CLI surface changes, so no `doc/*.scd` edit.
- **Environment.** The gates build the bundled DuckDB; on a macOS host whose default SDK cannot
  compile it, export `SDKROOT` to a working SDK first. `verify/red.sh` compiles each red tree into
  `target/red-target` (its first run rebuilds DuckDB there; later runs only the `xdu` crate). P2
  may fetch httpfs from `extensions.duckdb.org`.
- **The gate scripts** (`spec/xdu-view-prefix-bytes-vs-chars/verify/`): `scope.sh` holds R6/R7
  structurally (one production line changed, frozen copy intact); `red.sh TREE TOKEN TEST ARGS…`
  requires TEST to fail with TOKEN on `base` (`7689a92`) and on three one-line mutants of the fix.
  If a red run is green, the test is wrong, not the gate.
- **No fixture directory name may contain `_` or `%`**: `LIKE` treats them as wildcards (PLAN §4,
  [`issues/xdu-view-like-metacharacters.md`](../../issues/xdu-view-like-metacharacters.md)), which
  would make a listing pass or fail for a reason that is not this defect.

---

## Phase P1 — Count the prefix in code points; prove it locally, red on `7689a92`
**Satisfies:** R1, R2, R3, R4 (local), R5 (local), R6, R7 · **Depends on:** —
**Goal:** both views list full names under a non-ASCII local directory at every depth, the local
statement text stays pinned exactly, and the regression test is shown to fail on `main`.

- [x] `src/view_query.rs`, `DirectoryQuery::new`: exactly `let prefix_len = prefix.chars().count();`
      in place of the `prefix.len()` line (`scope.sh` allows that one changed line), with a
      one-line comment naming the unit (Unicode scalar values, what DuckDB's `substr` and
      `position` count). Say what the field counts in the struct's doc. No other statement change.
- [x] `mod tests`: leave `old_directory_parts` byte-identical to `main` (`scope.sh` checks it). In
      the pin, add the fourth directory case
      `("données", "/data/scratch/données/e\u{301}t\u{e9}/\u{FFFD}")`; compare each statement with
      `assert_eq!` against the frozen text with `substr(path, {bytes} + 1` replaced by
      `substr(path, {code points} + 1` (counts of `"{dir}/"`), asserting for the ASCII cases that
      the two counts are equal. Make the final line exactly `assert_eq!(checked, 3 * 7 * 4);`. Do
      not loosen the comparison.
- [x] `mod tests`: add `directory_offset_counts_code_points` (PLAN §2.2): `dir =
      "/d/é𝄞\u{FFFD}e\u{301}"`, assert `dir.len() + 1 == 16` and `dir.chars().count() + 1 == 9`
      as a guard on the fixture itself, then for both builders, both arms and every `SortMode` the
      text contains `substr(path, 9 + 1)` and not `16 + 1`; the S3 parameters are exactly
      `[glob, "<dir>/%"]`.
- [x] `tests/common/mod.rs`: the shared fixture (PLAN §2.3) — one partition's subtree with
      `alpha/résumé.txt`, `alpha/z`, `beta/f`, `top`, `e\u{301}t\u{e9}/a/deep.txt`,
      `bad\u{FFFD}name/b`, `clef\u{1D11E}/c`, distinct sizes, and the ASCII twin with the same
      shape, sizes and ages. Ages are absolute: add `set_atime_at(path, secs)` and set every file
      of both trees from one epoch taken once, never from two `now()` calls. Return the files (relative path, size) so
      the tests compare against data, never a re-derived cut.
- [x] `tests/view_tests.rs` (new, `mod common;`): `non_ascii_directories_list_full_names_in_both_views`
      per PLAN §2.3 — non-ASCII crawled root `hôme-ü` (spelled literally in the file; the gate
      greps it), partitions `données` and `p`; assert the
      canonical root and every listed directory are not ASCII; walk list and tree views from
      `partition_root`, every name assertion carrying the message
      `"local {view} full names under {dir}"`, the list view's first listing asserted before
      anything else; walked file set and sizes equal the fixture's; each
      listing matches the ASCII twin's mapped listing on `is_dir`, `total_size`, `file_count`,
      `latest_atime`.
- **Verify:** the frontmatter `verify:` (lib tests named ok; `view_tests` ok; `scope.sh`; the test
  red with `local list full names under` on `base` and the three mutants; full `cargo test`,
  clippy, fmt).
- **Touches:** `src/view_query.rs` (**high blast**), `tests/common/mod.rs`, `tests/view_tests.rs`.

## Phase P2 — The S3 arm: a wire test with explicit names; wire count 54; close the ledger
**Satisfies:** R4, R5 · **Depends on:** P1
**Goal:** the same walk passes over an `s3://` index with the non-ASCII directory above and below
the partition, the test is red on `7689a92`, and the standing CI gate counts it.

- [ ] `tests/s3_tests.rs`: `view_non_ascii_directories_list_full_names` (PLAN §2.4), `#[test]` and
      the siblings' exact `#[ignore = "…"]` reason; ASCII partition `p` only, under a non-ASCII
      crawled root; the explicit `"s3 {view} full names under {dir}"` walk over the S3 index
      before any local query, then `ViewParity::compare` over the walked directories, then
      `parity.finish()`. Reuse the P1 fixture and `set_atime_at`. No early
      `return` (the wire script refuses it).
- [ ] `.github/workflows/test.yaml`: `sh tests/s3-wire.sh "" 54`. `AGENTS.md`:
      `XDU_TEST_EXTENSION_DIR=target/test-ext sh tests/s3-wire.sh "" 54`. Same commit as the test.
- [ ] Deferral ledger: walk P1, P2 and PLAN for "not fixed here", "follow-up", "known limitation",
      "deferred". The one deferral this cycle makes, `LIKE` `_`/`%` over-matching, is recorded as
      `issues/xdu-view-like-metacharacters.md` with a `ROADMAP.md` entry (filed at plan). Any new
      one found while building gets its own `issues/` file and ROADMAP entry before this phase
      closes; never `META.md`.
- **Verify:** the frontmatter `verify:` (fetch httpfs if absent; the new test alone, the `view_`
  family, then the whole file at 54 with the listed count asserted; both count literals; the test
  red with `s3 list full names under` on `base` and the three mutants; the seed and its entry;
  `scope.sh` still green; full `cargo test`, clippy, fmt).
- **Touches:** `tests/s3_tests.rs`, `.github/workflows/test.yaml` (**CI gate**), `AGENTS.md`.

---

## How `xdu-build` drives this

1. `next_phase.py` prints the next actionable phase.
2. Pre-flight: clean tree, on `branch`, `base` reachable.
3. Execute every `[ ]` in the phase (PLAN.md for detail).
4. Run the phase's `verify:` command — never advance on a checkbox alone.
5. Amend this file if reality diverges (regenerate frontmatter with `set_phase.py`; note it in the
   commit body). STOP and escalate only on a `GOAL.md` contradiction.
6. Mark the phase `done`, advance `current_phase`, `--touch`; one `[fix]` commit; stop and report.
