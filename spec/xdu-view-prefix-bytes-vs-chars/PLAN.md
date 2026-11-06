# PLAN — `xdu-view` lists full child names under a non-ASCII directory

> **Status:** Draft for review; adversarial gate review folded in (§7) · **Last updated:** 2026-11-06
> **Authoritative technical design.** The *how*. Vision/contract is [`GOAL.md`](GOAL.md);
> the phased executable roadmap is [`TECH.md`](TECH.md). Lean path (`kind: fix`, `appetite:
> small`): no `research/`; the one external unknown, DuckDB's string unit, was settled by
> execution (§4). Every design element traces to a GOAL R-ID.

## 1. Summary

`DirectoryQuery::new` measures the directory prefix with `prefix.chars().count()` instead of
`prefix.len()`: the number of Unicode scalar values, the unit the bundled DuckDB 1.4.4's `substr`
and `position` count (§4, executed). That is the whole code change, one line in
`src/view_query.rs`; the statement text is untouched apart from the integer it carries, so every
ASCII listing sends byte-identical SQL. The rest of the cycle is proof: the local-statement pin
moves deliberately and gains a non-ASCII case, a unit test pins the offset as a literal, a local
integration test walks both views through a tree with non-ASCII at every level, an S3 wire test
does the same through the bucket, and each new test is shown red on `main` `7689a92` and on three one-line mutants of the fix by a
gate script, so the walk provably reaches the decomposed, U+FFFD and 4-byte directories. Two
phases: local fix and proof, then the S3 arm.

## 2. Design

### 2.1 The offset (`src/view_query.rs`, `DirectoryQuery::new`)

```rust
let prefix = format!("{}/", dir);
// Unicode scalar values: the unit DuckDB's substr and position count (not bytes, not graphemes).
let prefix_len = prefix.chars().count();
```

The field keeps its name and type (`usize`) and the doc comment on `DirectoryQuery` says what it
counts. Nothing else in the two builders changes. Why this is enough, given the statement:

- `substr(path, {prefix_len} + 1)` starts the cut at code point `prefix_len + 1`. The `LIKE` that
  selected the row guarantees the row begins with the prefix's code points, so the cut lands
  exactly after the prefix.
- `position('/' IN substr(...))` returns a code-point index into the remainder, and it is fed back
  as the length argument of the outer `substr`, which counts code points too. Both sides of that
  inner pair already agreed on `main`; a non-ASCII *child* name was only ever damaged by the
  outer offset (§4 measures `position`).
- A decomposed name (`e` + U+0301) is two code points to both DuckDB and `chars()`, and a stored
  U+FFFD is one; the cut is by code point, never through one, so no grapheme or lossy name can be
  split mid-character.

The two other options in GOAL's first clarification are rejected by the foreman's ruling (code
points, no binding) and on their own merits: `length(<prefix>)` in SQL would write the local prefix
into a second literal (a §5 widening GOAL forbids) or bind it a second time on S3 (a new parameter
position for no gain); a prefix-stripping function needs the prefix as a value in the same way.

### 2.2 Pinning the text (`src/view_query.rs`, `mod tests`)

- `local_view_sql_is_byte_for_byte_what_xdu_view_sent` stays an exact comparison (R6), and its
  frozen `old_directory_parts` stays byte-identical to `main`'s (the scope gate checks it), so the
  pin keeps comparing against what `xdu-view` sent. The directory list gains a fourth case under a
  non-ASCII partition, `("données", "/data/scratch/données/e\u{301}t\u{e9}/\u{FFFD}")`. For every
  case the expected text is the frozen text with `substr(path, {bytes} + 1` replaced by
  `substr(path, {code points} + 1` (counts of `"{dir}/"`); for the three ASCII cases the test first
  asserts the two counts are equal, so their expected text is `main`'s verbatim. The final count
  becomes `assert_eq!(checked, 3 * 7 * 4);`. The pin is still `assert_eq!` on whole statements:
  the one number is the only permitted difference from `main`.
- New `directory_offset_counts_code_points`: a frozen copy that moves with the builder cannot catch
  both reverting to bytes together, so this test states the number. For
  `dir = "/d/é𝄞\u{FFFD}e\u{301}"` the prefix is 9 code points and 16 bytes; for both builders, both
  arms (local and S3) and every sort mode the text contains `substr(path, 9 + 1)` and not
  `16 + 1`, and the S3 parameters are unchanged (`[glob, "<dir>/%"]`).
- `s3_view_sql_binds_the_glob_and_the_directory_prefix` is untouched and must pass (R7).

### 2.3 Local regression test (`tests/view_tests.rs`, new; fixture in `tests/common/mod.rs`)

One integration test, `non_ascii_directories_list_full_names_in_both_views`, calling the public
`xdu::view_query` API (`partition_root`, `list_directory`, `tree_directory`) against indexes the
real `xdu` binary builds with `common::build_index`.

The fixture lives in `tests/common/mod.rs` (AGENTS.md: the one home for fixtures) because the S3
test reuses it: `non_ascii_view_tree(root: &Path, partition: &str) -> Vec<(String, usize)>`
creates one partition's subtree and returns each file's path relative to `root` with its size;
an `ascii` twin flag (or a sibling function) builds the same shape with every non-ASCII component
mapped to ASCII. Access times are absolute: one epoch is taken once per test and each file gets
`epoch - k * 86400` through a new `set_atime_at(path, secs)` helper beside `set_atime_days_ago`, so
the twin's `latest_atime` cannot differ by a second boundary crossed between the two builds. The
shape, inside the partition:

```
alpha/résumé.txt       non-ASCII leaf name (the position() side)
alpha/z                one-character name, shorter than any overshoot (would vanish)
beta/f
top
e\u{301}t\u{e9}/a/deep.txt   decomposed + composed in one directory name, two levels down
bad\u{FFFD}name/b      U+FFFD, as a lossy crawl stores it
clef\u{1D11E}/c        a 4-byte code point
```

Files get distinct sizes and ages so sizes and latest access times are comparable. U+FFFD is written as the character itself: the index stores exactly that string for
a lossily converted name, and APFS refuses the invalid bytes a real lossy name would need, so a
literal U+FFFD is the platform-independent way to put the stored form in the index (GOAL non-goal:
recovering original bytes). NFC and NFD spellings are never siblings, since APFS would fold them.

The test:

1. Builds the source under a crawled root that is itself non-ASCII (`<tmp>/hôme-ü/src`), with two
   partitions, `données` and `p`, each from the fixture; builds the ASCII twin under
   `<tmp>/home-u/src` with partitions `donnees` and `p`. Asserts the canonical crawled root is not
   ASCII (so partition `p`'s listings carry non-ASCII only from above the partition) and that every
   listed directory is not ASCII, so the fixture cannot silently degrade to an ASCII tree.
2. For each partition, from `partition_root` down, **walks** each view: list a directory, assert
   its names equal the expected set (message `"local {view} full names under {dir}"`, the first
   assertion the test makes, list view first), then descend into every `is_dir` entry by its `path`. It collects
   every file entry's `path` and size. The walked file set must equal the fixture's file set
   exactly (R1, R2 at every depth; a truncated name leads to an empty directory and fails here).
3. Compares each listing with the ASCII twin's listing of the mapped directory: same entries
   after mapping names, same `is_dir`, `total_size`, `file_count`, `latest_atime` (R1).

On `7689a92` step 2 fails on its first listing, with the expected names on one side and truncated
ones on the other. `verify/red.sh base` proves that by running this branch's `tests/` against
that commit's source.

### 2.4 S3 wire test (`tests/s3_tests.rs`)

`view_non_ascii_directories_list_full_names`, `#[ignore]`d with the siblings' reason string, in the
`view_` family. It builds the fixture under a non-ASCII crawled root with the ASCII partition `p`
only (the writer refuses a non-ASCII partition name), seeds it with `TestS3::seed_index`, and opens
it through `ViewParity::over`. It then runs the same walk over the S3 index **before touching the
local one**, with `"s3 {view} full names under {dir}"` assertions first, so the non-ASCII directory is both above the
partition (the crawled root) and below it (R4). Parity with the local index over every walked
directory (`ViewParity::compare` with those directories) and `parity.finish()` follow. Parity
alone would pass on `main`, since both arms truncate alike; the explicit names are what make the
test red there, and the arm tag in the message lets the red gate require that the S3 arm, not the
local one, is what failed.

The wire suite's listed count goes from 53 to **54**: `.github/workflows/test.yaml` (`sh
tests/s3-wire.sh "" 54`) and `AGENTS.md`'s command line (`XDU_TEST_EXTENSION_DIR=target/test-ext
sh tests/s3-wire.sh "" 54`) move in the same commit as the test (AGENTS.md, Testing).

### 2.5 Files to touch

| File | Change | Blast |
|---|---|---|
| `src/view_query.rs` | the offset line; the pin's frozen copy and fourth case; the offset unit test | **high** (invariants.md: every `xdu-view` statement) |
| `tests/common/mod.rs` | the shared fixture (and the twin), `set_atime_at` | medium: every test target compiles it |
| `tests/view_tests.rs` | new: the local regression test | low |
| `tests/s3_tests.rs` | the wire test | low |
| `.github/workflows/test.yaml` | wire count 53 → 54 | **CI gate** |
| `AGENTS.md` | wire count 53 → 54 on the Testing command line | medium (the constitution) |

`invariants.md` §5's `xdu-view` bullet still describes the code exactly (local `LIKE` literal, S3
bound prefix) and needs no edit.

### Requirement → design map

| R-ID | Design element(s) that satisfy it |
|------|-----------------------------------|
| R1 | §2.1 offset; §2.3 walk names + twin comparison, list and tree |
| R2 | §2.3 / §2.4 walk descends by each entry's `path` to every file |
| R3 | §2.3 fixture: decomposed name, U+FFFD, 4-byte code point, non-ASCII leaf; §2.2 offset test |
| R4 | §2.3 local (root, partition, below); §2.4 S3 (root above, directories below `p`) |
| R5 | §2.3 / §2.4 tests through `xdu::view_query` on `xdu`-built indexes; `verify/red.sh` red on `7689a92` and on three mutants |
| R6 | §2.2 exact pin against `main`'s frozen text, only the offset number swapped; `verify/scope.sh`; full `cargo test`; existing `view_` wire tests |
| R7 | §2.1 integer only; §2.2 S3 binding test untouched; `verify/scope.sh`: production `view_query.rs` differs from `7689a92` by the one offset line, no other `src/` file changes |

## 3. Invariant gate (AGENTS.md constitution check)

Checked against [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) before the
design (Step 2) and again after it (Step 5). Touched:

- **§5 DuckDB injection surface** — the only new SQL text is an integer computed in Rust, safe by
  type. The local prefix stays in its existing `LIKE` literal (the recorded exception, owned by
  `local-reader-bound-parameters`); the S3 arm still binds the glob and the prefix, in the same
  order. No string reaches a new literal. The pin test and the S3 binding test both run in P1.
- **§11 Altitude / testability** — the change is in the library's pure `*_sql` builder; tests reach
  it through the public API, and no test reimplements the cut.
- **§12 TUI terminal safety** — not touched: `src/bin/xdu-view.rs` is out of scope (GOAL non-goals;
  its display slicing compares byte lengths on both sides). Display width of multibyte names stays
  with `xdu-view-terminal-safety`.
- **§13 Conventions** — no CLI surface, so no `doc/*.scd` change. The wire count moves with the
  test in one commit.
- **High-blast list** — `src/view_query.rs` is on it; any CONFIRMED review finding there takes the
  human gate.

Not touched: §1 schema, §2/§2b/§2c finalization and markers, §3 partition scheme, §4 `xdu-rm`, §6,
§7, §8, §9 (`ORDER BY` text unchanged), §10.

### Deviation justifications

| Deviation | Why needed | Simpler alternative rejected because |
|-----------|-----------|--------------------------------------|
| — | — | — |

## 4. Rabbit holes (resolved)

- **What unit do the bundled engine's string functions count?** Settled by execution on the engine
  the `duckdb` crate bundles (`SELECT version()` → `v1.4.4`), through `xdu::engine::open_connection`,
  in a throwaway probe crate under the untracked `target/`. Each value was bound and compared with
  Rust's counts:

  ```
  engine=v1.4.4
  "données/":       length=8 chars().count()=8 strlen=9  len()=9  substr(s||'child/x', length(s)+1)="child/x" position('/')=8
  "de\u{301}cor/":  length=7 chars().count()=7 strlen=8  len()=8  substr(...)="child/x" position('/')=7
  "bad\u{FFFD}name/": length=9 chars().count()=9 strlen=11 len()=11 substr(...)="child/x" position('/')=9
  "clef\u{1D11E}/": length=6 chars().count()=6 strlen=9  len()=9  substr(...)="child/x" position('/')=6
  "plain/":         length=6 chars().count()=6 strlen=6  len()=6  substr(...)="child/x" position('/')=6
  byte offset 12 -> "lpha/f"; code-point offset 11 -> "alpha/f"
  ```

  `length`, `substr` and `position` count code points and agree with `chars().count()`, including
  for a combining mark (not graphemes), U+FFFD and a 4-byte scalar; `strlen` counts bytes. The
  same probe listed `<root>/données` through `list_directory` and `tree_directory` on an index
  `xdu` built and got `["eta", "lpha", "op"]` from both, reproducing the seed on 1.4.4.
- **Does `LIKE` `_`/`%` over-match on 1.4.4?** Yes. Same probe, partition `p` holding `a_b/f1`,
  `axb/g1`, `a%b/h1`, `aZZb/g2`: `'/r/p/axb/g1' LIKE '/r/p/a_b/%'` and
  `'/r/p/aZZb/g2' LIKE '/r/p/a%b/%'` are both true, and listing `a_b` (and `a%b`) through both views
  returns `["f1", "g1", "h1"]`. Filed per the foreman's ruling as
  [`issues/xdu-view-like-metacharacters.md`](../../issues/xdu-view-like-metacharacters.md) with a
  `ROADMAP.md` entry; not fixed here, and the code-point offset neither closes nor widens it.

## 5. Risks & open questions

- **The red gate builds other trees.** `verify/red.sh` extracts each tree (base, or this branch
  with one mutated line) into a fresh directory under `target/red-trees/` and compiles it into
  `target/red-target`, so the first run compiles the bundled DuckDB again and later runs rebuild
  only the `xdu` crate. It must not share `target/`: a red tree's binaries would replace the ones
  `CARGO_BIN_EXE_*` names for this branch's gates. Exercised at plan time: on the pre-fix tree it
  extracted `7689a92`, built it, and refused a test that passes there.
- **The S3 gate needs the pinned httpfs.** P2 fetches it into `target/test-ext` when absent, the
  AGENTS.md way (`cargo build --locked --example extension-probe`, then
  `sh packaging/extensions/fetch.sh target/debug/examples/extension-probe target/test-ext`), which
  reaches `extensions.duckdb.org`. Never into `target/lib/xdu/extensions` (`s3-wire.sh` refuses it).
- **Filesystem normalization.** APFS preserves NFD names and refuses invalid UTF-8; the fixture
  relies on the first and avoids the second (§2.3). On Linux both forms are stored as given.
- **`LIKE` over-matching stays.** A fixture directory name must not contain `_` or `%`, or the
  listing would include siblings for the reason the new seed records, not this one.

## 6. Verification strategy

- P1: `cargo test --locked --lib view_query::` with the pin, the offset test and the S3 binding
  test named ok; `cargo test --locked --test view_tests` with the regression test named ok;
  `verify/scope.sh`; `verify/red.sh` showing the regression test red, with
  `local list full names under`, on `base`, `mutant-utf16`, `mutant-nomarks` and `mutant-nofffd`;
  full `cargo test --locked`, clippy, fmt.
- P2: fetch httpfs if absent; `tests/s3-wire.sh view_non_ascii 1`, then `view_ 6`, then the whole
  file at `54`, with the listed count asserted equal to 54 and both literals updated; the wire test
  red, with `s3 list full names under`, on the same four trees; the LIKE seed and its ROADMAP entry
  present (deferral ledger).
- The mutants are why the walk's depth is gated: each miscounts only past one kind of character
  (a 4-byte scalar, a combining mark, U+FFFD), so it turns the test red only if the walk lists a
  directory below such a name. `base` alone cannot show that, since the non-ASCII crawled root
  breaks the very first listing.
- There is no CLI drive: `xdu-view` is an interactive TUI, and the statements it sends are the
  library functions these tests call (the binary only renders `DirEntry`). The indexes are built by
  the real `xdu` binary.

## 7. Adversarial review of the gates (Step 3, gate-shaped deliverables)

One read-only reviewer was asked for inputs the drafted gates pass while broken or fail while
good. All seven findings were adopted:

1. *Red on base cannot show depth* (the non-ASCII root breaks the first listing) → the three
   mutants in `verify/red.sh`.
2. *Twin `latest_atime` could flake across a second boundary* → absolute atimes from one epoch
   (`set_atime_at`).
3. *The `'{` diff grep was narrow, could pass when `git diff` failed, and could trip on tests* →
   `verify/scope.sh` compares the production half of `view_query.rs` (above `#[cfg(test)]`,
   comments dropped) with `7689a92` and allows only the offset line; no other `src/` file may
   change.
4. *The S3 red run could fail on a local assertion* → arm-tagged messages, the token passed to the
   gate, and the S3 test walks S3 first.
5. *The `prefix.len()` sed check could match nothing* → subsumed by `scope.sh`'s exact diff and its
   positive grep for the `9 + 1` literal.
6. *The pin could be loosened* → the frozen copy stays byte-identical to `main` (checked), the
   expected text swaps only the offset number, and the case-count grep is anchored to live code.
7. *Stale files in the red tree* → every run extracts into a fresh directory.

Confirmed sound by the same review: binary isolation through a separate target dir, the S3 red
run's reach to the in-process endpoint, the wire counts (53 today, `view_` 5 → 6), the parsed
`verify:` strings, and the §2.1 semantics (`position`/`substr` agree; empty and NULL components
are already filtered).
