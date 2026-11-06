# REVIEW — `xdu-view` lists full child names under a non-ASCII directory

> Adversarial QA by `xdu-review`, run in an isolated/clean context. The correctness pass grades the
> branch diff against [`GOAL.md`](GOAL.md) + the AGENTS.md invariants **only** — it does not see
> `PLAN.md`/`TECH.md` (avoids grading-its-own-homework / plan-sycophancy). Every finding cites an
> **executed** command, not an assertion.

- **Reviewed commit:** `30b7f69`  ·  **Base:** `main` (`7689a92`)  ·  **Date:** 2026-11-06
- **Verdict:** approved
- **Cycle:** 1 of ≤3 — mirrors `review.cycle` in `TECH.md` (escalate to human on non-convergence)

Mode: standard, one blind reviewer. It was given GOAL.md, the diff with `spec/` excluded,
`review-rubric.md` and `invariants.md`, and was barred from PLAN, TECH, META and `spec/`. The
completeness sub-pass was skipped at the foreman's direction. Contract drift: `fd022bf` records
the foreman's GOAL-gate rulings after the shaping commit `3a5071e`. The foreman approved it, and
this review grades GOAL.md as it stands at `30b7f69`.

## Verification run

**The blind reviewer** worked in its own `git archive` export of `30b7f69` with its own target
dir, using a throwaway probe test that calls `xdu::view_query` against indexes the real `xdu`
builds:

- `cargo test --locked`: green (lib 170, `view_tests` 1, crawl 25, rm 19, find 9, s3 non-wire 4,
  and the rest). `cargo fmt --all -- --check` and
  `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- **Overshoot probe.** A crawled root of `ééééééééé🎉🎉🎉` overshoots a byte offset by 18, and
  holds children `a` and `bb`, shorter than the overshoot. Both views were walked at every depth,
  under four filter sets (none, size+age, glob, regex+newer) and all seven sort modes, against
  an ASCII twin: `checked 518 entries ... ok`.
- **Engine units.** Bundled engine `v1.4.4`. `length` equals `chars().count()`, `strlen` equals
  `len()`, and `length_grapheme` differs from both, for a decomposed `é`, U+FFFD, `𝄞`, a ZWJ
  family emoji, a flag followed by CRLF, and Hangul jamo. One example:
  `"/d/👨‍👩‍👧/ab/c": length=13 rust_chars=13 strlen=26 grapheme=9`. `substr` at
  `chars().count(prefix) + 1` returned exactly the tail.
- **Red on main (R5).** It swapped in main's `src/view_query.rs`, `cmp`-equal to `7689a92`'s:
  - `view_tests` FAILED, `local list full names under …/hôme-ü/src/données`, with
    `left: ["a", "f𝄞", "ha", "é", "�name"]`.
  - The S3 wire test FAILED, `s3 list full names under …/hôme-ü/src/p`, with
    `left: ["d", "d�name", "ef𝄞", "p", "pha", "ta", "té"]`.
  - It restored the file, verified it by `cmp` against `HEAD`, rebuilt, and reran: `view_tests` ok,
    wire tier 54 of 54.
- **ASCII parity (R6).** All 84 ASCII listings from main and from the fix hold the same entries
  once each listing is sorted.
- `tests/s3-wire.sh "view_non_ascii" 1`: `1 of 1 passed`. `tests/s3-wire.sh "" 54`:
  `54 of 54 passed`.

**The orchestrator** ran these in a separate export of `30b7f69` with its own target dir:

- **Engine units.** A probe through `xdu::engine::open_connection` printed `engine=v1.4.4`. For
  `données/`, `de\u{301}cor/`, `bad\u{FFFD}name/`, `clef\u{1D11E}/` and `plain/`:
  - `length` and `position('/')` equal `chars().count()`, and `strlen` equals the byte length.
  - `substr(s || 'chéld/x', chars + 1)` returns `chéld/x`.
  - The views' `position` arithmetic cuts that tail to the five code points of `chéld`.
- `cargo test --locked --lib view_query::`: three tests ok,
  `local_view_sql_is_byte_for_byte_what_xdu_view_sent`, `directory_offset_counts_code_points`
  and `s3_view_sql_binds_the_glob_and_the_directory_prefix`.
- `cargo test --locked`: every suite ok, with the 50 wire tests ignored, as designed.
- `XDU_TEST_EXTENSION_DIR=<abs>/target/test-ext sh tests/s3-wire.sh "" 54`:
  `s3-wire: '' 54 of 54 passed`, with `view_non_ascii_directories_list_full_names ... ok`.
- `cargo fmt --all -- --check`: clean. The added lines under `src/` and `tests/` carry no
  `R#`/`P#` id (`git diff main...HEAD -- src tests | grep '^+' | grep -cE '\b[RP][0-9]+\b'` → 0).
- **Red on main (R5), by the branch's own gate.** `verify/red.sh base` was run from a local clone,
  for both tests:
  - `view_tests` failed at the first listing, `…/hôme-ü/src/données`.
  - The S3 wire test failed under `…/hôme-ü/src/p` with the same `left` as above.
  - `red.sh mutant-nofffd` failed only under `…/données/bad�name`.
- **Not observed here:** the man-page gate. The diff touches no `doc/*.scd`. The foreman reports
  CI on `30b7f69` green: Tests run 42297853541, all four jobs. On both ubuntu-24.04 and macos-15
  it shows `s3-wire: '' 54 of 54 passed` and
  `non_ascii_directories_list_full_names_in_both_views ... ok`.

### The red gate (orchestrator only; harness, not product code)

`verify/red.sh` was hardened during the build after a stale-build false pass (META F3). It
stamps every extracted file before building, and it requires `Compiling xdu v` in the log.
Executed check:

- **As shipped, sound.** Above, `base` restamped the `libxdu` rlib and failed where the defect
  is, at the first non-ASCII listing.
- **Control.** Only the `touch` line was removed, in a scratch copy, and the script ran
  `mutant-nofffd` then `base`. The `base` run reused the mutant's library: the rlib's mtime did
  not move, and the failure came under `…/bad�name`, the mutant's signature. It still printed
  `red: … fails on base`. Its log held `Compiling xdu v0.6.0 (…/base.*/src)`: the `tests/`
  overlay is copied with fresh mtimes, so the test targets always recompile.

The stamp alone carries the gate, and the `Compiling xdu v` guard cannot fail in the case it
was added for. That makes no difference to this branch's verdict, because the shipped script
stamps. It is recorded as META F4 against F3's recommended fix. The script is a build-phase
record under `spec/`, not a standing gate.

## Requirement → evidence matrix

A = the blind reviewer, O = the orchestrator.

| R-ID | Implemented by (file/commit) | Verified how | By | Status |
|------|------------------------------|--------------|----|--------|
| R1 | `src/view_query.rs` `DirectoryQuery::new`, `prefix.chars().count()` (`304af3f`) | `non_ascii_directories_list_full_names_in_both_views` ok: non-ASCII root, partition and deeper directories, with sizes, counts and atimes checked against an ASCII twin. The overshoot probe checked 518 entries over 4 filter sets × 7 sorts. | A, O | ✅ |
| R2 | same | `walk_view` enters every listed directory by its `path`, in the local and the wire test. The probe went 3 levels below the partition. | A, O | ✅ |
| R3 | same | Engine units on `v1.4.4`, run by both. The fixture names `e\u{301}té`, `bad\u{FFFD}name` and `clef𝄞` are listed and entered. | A, O | ✅ |
| R4 | same; `tests/s3_tests.rs` `view_non_ascii_directories_list_full_names` (`30b7f69`) | `s3-wire.sh "" 54`: 54 of 54. Non-ASCII sits above the partition (`hôme-ü`) and below it, with parity against local in every sort mode. | A, O | ✅ |
| R5 | `tests/view_tests.rs`, `tests/common/mod.rs` (`304af3f`); the wire test, with the count 54 in CI and AGENTS.md (`30b7f69`) | Both tests red on main's `view_query.rs` (A by swap-in, O by `red.sh base`) and green with the fix. | A, O | ✅ |
| R6 | the pin in `src/view_query.rs` `mod tests`, now 3 × 7 × 4 cases | The pin is exact: the identity for ASCII, with a 5-occurrence assertion before the non-ASCII offset rewrite, and the frozen builders unchanged. 84 ASCII listings match main. Full `cargo test` green. | A, O | ✅ |
| R7 | the only SQL change is the integer `prefix_len` | Read from the diff. `directory_offset_counts_code_points` asserts the S3 params stay `[glob, "{dir}/%"]` and the local params empty. | A, O | ✅ |
| GOAL clarification: confirm the units on the pinned 1.4.4 by execution | PLAN §4, a committed artifact under `spec/` | Read the recorded probe output in PLAN §4, and re-ran an independent probe on `v1.4.4`. | O | ✅ |
| GOAL clarification: file `LIKE` `_`/`%` as a seed with a ROADMAP entry | `issues/xdu-view-like-metacharacters.md`, `ROADMAP.md` (`09253e5`) | The seed holds `## Problem` and a reproduction, and ROADMAP links it. | A, O | ✅ |

Unmapped changes (possible scope creep): none. The seed's `status:` line in
`issues/xdu-view-prefix-bytes-vs-chars.md` is lifecycle bookkeeping. The wire count 53 → 54 in
`.github/workflows/test.yaml` and `AGENTS.md` is R5's "on the same terms as its siblings".

Operating-manual drift: none rated. `AGENTS.md`'s `tests/` map does not name the new
`tests/view_tests.rs`, but on main it already omits `find_tests.rs`, `cli_version_tests.rs` and
`version_tests.rs`, so the map is selective. `invariants.md` §5's `xdu-view` bullet still holds:
the local arm keeps the `LIKE` literal, the S3 arm binds the prefix, and the named tests are
unchanged.

## Findings

None. The reviewer refuted and dropped two candidates:

- **A `\r\n` directory name under a `*.txt` filter.** It dropped from the probe's listing. The
  cause is the shared `QueryFilters` regex, where `.` does not match `\n`, and is not the offset.
  The probe's twin was not like-for-like, and a `\r`-only name passes.
- **ASCII order differences between main and the fix.** They occur only among entries with equal
  sort keys, such as count ties under `CountAsc`. Within one run, list and tree already order
  such ties differently. The entries themselves are identical.

## Human-gate triggers

- Not triggered: no CONFIRMED finding. The diff changes `src/view_query.rs`, a high-blast file,
  but no finding touches it or an `invariants.md` §1/§2/§4/§5 invariant.

## Optional completeness sub-pass (separate reviewer; may see TECH.md)

- Not run (skipped at the foreman's direction).
