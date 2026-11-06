# 04 — Packaging touchpoints for `xdu-cp` and `xdu-mv` (R10, §13)

Method: `grep -rn xdu-rm` over the tree minus `spec/`, `target/`, `.git/`, `.worktrees/` (59 files);
each hit classified as packaging list, doc cross-reference, prose count, or unrelated (factory
templates, rubric and `ears.md` cite `xdu-rm` only as the destructive example, so they need no change).

## Automatic, no edit needed

`cargo build --bins` in `release.yaml:155,273` and `temp_index.sh:35`; the man render loop
`for scd in doc/*.scd` (`test.yaml:158`, `release.yaml:83`); the roff control-line tripwire
(`test.yaml:293`, globs `doc/*.scd`); `install.sh:218,226,234` (globs `share/`);
`SHA256SUMS` (`release.yaml:350`, globs tarballs); `temp_index.sh` puts all of `target/release` on
PATH. For a cp/mv drive, `--into` must point under the site (the cwd), and an mv drive moves fixture
files, which is fine because the site is thrown away.

## Must change (lockstep set)

| File:line | Change | CI-checked? | Blast radius |
|---|---|---|---|
| `Cargo.toml:65-67` | two `[[bin]]` blocks | build | none |
| `src/cli.rs:199-210` pattern | `XduCpArgs`, `XduMvArgs` with `name`, `version`, `after_help` | only via the version test | **high-blast file** (any confirmed finding forces a human gate) |
| `src/bin/gen-completions.rs:11,29-34` | import + two tuples | only if `test.yaml` "produced" list is extended | silent: no completions ship |
| `tests/common/mod.rs:25-31` | two `binary_path` arms | compile (`CARGO_BIN_EXE_*` exists only with the `[[bin]]`) | none |
| `tests/cli_version_tests.rs:19` | add both to the literal array | no: the array is hand-kept | a bin missing `version` goes unnoticed |
| `tests/offline_tests.rs:61-80` | optional cold-cache `xdu-cp --dry-run` | n/a | module doc (`:16-19`) argues linkage is crate-wide; add one only if the engine opens DuckDB differently |
| `.github/workflows/release.yaml:168` | glibc-floor + `NEEDED` allow-list loop (Linux legs) | **tag-time only** | an unchecked bin can ship over the floor |
| `release.yaml:208` (Linux), `:289` (macOS) | tarball assembly loops | tag-time only; caught only if `check.sh` is updated | tarball silently lacks the bins |
| `packaging/extensions/check.sh:49,100,108` | presence, engine `source_id`, `--version` loops; optional dry-run smoke beside `:122-124` | runs in release legs **and** the `Dockerfile` build | **shared by two callers**: change it and the `Dockerfile` in one commit |
| `Dockerfile:3,52,54-58` (+ "four" comment near `:85`) | `--bin` list, `cp` list, comments | yes at PR time: `docker.yaml:25-33` path filter includes `Dockerfile`, `Cargo.toml`, `packaging/extensions/**` | an image build fails if `check.sh` expects bins the `Dockerfile` did not copy |
| `.github/workflows/docker.yaml:9` | "four xdu Rust binaries" comment | no | cosmetic |
| `.github/workflows/test.yaml:291` | two `check` lines (below) | it *is* the gate | a new page is otherwise **unasserted** |
| `test.yaml:324-345` | +2 man, +2 bash, +2 zsh paths; "all four" in the OK echo | it is the gate | silent otherwise |
| `install.sh:16` | `BINARIES` | no | low: the `:185` loop skips absent bins, so the site's copy (deployed on push to `main`, `deploy-pages.yaml:32`) stays safe against older tarballs, and it also hides a tarball that lacks a bin |
| `hpccm/xdu.py:91,103-106` → `make -B` → `xdu.def:49-52`, `xdu.docker:30-33` | smoke `--version` for both | `make check` (`test.yaml:316-322`) checks only that the specs are fresh, and **never builds the image** | **see Risk 1** |
| `hpccm/README.md:35-36` | "four binaries" list | no | doc |
| `packaging/rpm/xdu.spec:22,28,~37` | `--bin`, `install`, `%files`, the "four binaries" comment | no workflow runs it (`AGENTS.md:345`); spec is at 0.4.2 | low; keep in lockstep anyway |
| `build.sh:8,44` | output list | no | pre-existing drift (already omits `xdu-rm`); optional |

## Docs and prose

- **Man cross-references:** SEE ALSO in `doc/xdu.1.scd:266`, `xdu-find.1.scd:222`,
  `xdu-view.1.scd:259`, `xdu-rm.1.scd:113`; the marker-warning list `xdu.1.scd:257-258` (if cp/mv
  share the §2b gates, which they should); `xdu.1.scd:230` (a lossy path "cannot be removed with
  xdu-rm"; it is now missing for cp/mv too); `xdu-find.1.scd:105` ("xdu-rm(1) refuses S3 indices").
  None of these touch an asserted literal unless the new text names `XDU_*`, `AWS_*` or
  `s3://BUCKET/PREFIX`. Re-count if it does. `doc/xdu-rm.1.scd:113` sits against the non-goal
  "every change to xdu-rm". It is doc only, but the plan should rule on it explicitly.
- `src/bin/xdu.rs:388` warning text ("will not round-trip to xdu-rm"): optional widening. No test
  pins the string. The comments at `src/lib.rs:328` and `src/crawl.rs:4` are optional.
- `README.md` (after `:277` "Bulk Deletion"): R10 requires one copy and one move example. Not CI-checked.
- `www/index.html:449` (tools table), `:461` ("all four"), `:893` († v0.8 roadmap entry). The site
  deploys on push to `main`, so it would advertise the tools before any release ships them. Keep the
  † marker until the tag, or edit at release.
- **`AGENTS.md`:** `:25` ("Five binaries… four user-facing" → seven/six) and the table, `:54`
  ("All four `#[command]`"), `:289` ("names four pages"), repo map `:326`, data-flow `:389`/`:439`,
  CLI surface `:457`, tarball layout `:658`.
- **`invariants.md` §13:** "All four `#[command(...)]` blocks", "the `check` list names four pages",
  tarball `bin/{…}`. §13 makes *adding a literal* a three-place same-commit obligation (`test.yaml`,
  `AGENTS.md` Commands, §13). The existing prose covers new `2x:XDU_*` counts generically. A new
  *class* of literal (the path mapping below) should be named in both prose places. Consider adding
  the engine module and `xdu-mv` to the high-blast list (`invariants.md:19-28`): `mv` unlinks sources.
- `.agents/skills/xdu-release/SKILL.md:162-183` ("diff shows only version lines"): needs an
  amendment if the recipe gates the smoke by version (Risk 1).

## `issues/` named by GOAL (all exist)

- `bulk-copy-move-xdu-cp-mv.md`: status already "shaped on feature/…". Retire it at publish with
  its ROADMAP pair (`ROADMAP.md:205-217`) via `/xdu-roadmap`. `/xdu-publish` only warns.
- `xdu-rm-on-shared-engine.md`: filed at the GOAL gate, ROADMAP `:244-253` present. No edit.
- `s3-index-rm.md`: names only `xdu-rm` (`:7-56`). Widen it at publish to the engine, `xdu-cp`, `xdu-mv`.
- `local-reader-bound-parameters.md`: names `xdu-rm` at `:17,24,41,58,65`. Narrow it to `xdu-find`
  and `xdu-view`, pointing `xdu-rm`'s share at `xdu-rm-on-shared-engine.md`.
- `xdu-tar-slice-archive.md`: GOAL implies no edit.
- `manpage-gate-coverage-gaps.md` (not named by GOAL): **update it**. (a)/(b) say "four pages", which
  becomes six. Note that this cycle hand-added two pages, which is exactly gap (b) being exercised.
  The (c) census ("1 on xdu-find.1…") is already stale. Mirror the same counts in `ROADMAP.md:430-438`.

## Literal-assert recommendation (`test.yaml`, after `:291`)

For each page, both counted, assuming the page follows `xdu-rm.1`'s pattern (a flag cross-reference
plus an ENVIRONMENT entry):

```
check share/man/man1/xdu-cp.1 '2x:XDU_INDEX' '2x:XDU_JOBS' '1x:s3://BUCKET/PREFIX' '<N>x:<mapping>'
check share/man/man1/xdu-mv.1 '2x:XDU_INDEX' '2x:XDU_JOBS' '1x:s3://BUCKET/PREFIX' '<N>x:<mapping>'
```

- `s3://BUCKET/PREFIX` applies only if the `-i` text copies rm's refusal sentence. The S3 refusal is
  R3, so it is worth stating.
- **A path-mapping literal** closes the R3 residue of gap (c) for the new pages: one worked
  `--root`/`--into` example rendered as concrete paths, e.g. `/scratch/alice/a.log` under
  `--root /scratch --into /archive` → `/archive/alice/a.log`. Assert the destination path by
  count. Avoid `_` and `*` in it, since those are scdoc markup, and keep it specific enough that
  whitespace fusion cannot synthesize it. Use distinct example paths on the two pages and the
  literal doubles as a page-identity check (gap (a)).
- Do not assert R10's atime and index-not-updated prose. Prose needles are brittle, and the feature's
  `verify/` harness should check that text instead.
- The counts are hand-derived. `scdoc` is not installed on this host, so derive them on an
  ubuntu-24.04 render (scdoc 1.11.2, the CI package) and re-count after any cross-reference edit.

## Risks

1. **hpccm smoke vs the pinned release.** The recipe installs `VERSION = '0.6.0'`, whose tarball
   has no `xdu-cp`. Adding the two `--version` lines unconditionally keeps `make check` green and
   breaks every image build until the bump. Option A: gate them like `FIRST_WITH_LIB`
   (`xdu.py:46,110`). The constant names a release number not yet decided, and the bump commit then
   gains spec lines, so `xdu-release` steps 3/5 need a sentence like the extension guard's.
   Option B: smoke every `bin/xdu*` by glob. That is version-independent and leaves presence to
   `check.sh`.
2. **Release loops are tag-time only.** `release.yaml:168,208,289` are never exercised on a PR. The
   `check.sh` presence loop is the only backstop, and it is PR-exercised only through the
   `Dockerfile` path filter.
3. `install.sh`'s skip-if-absent loop masks a tarball missing a binary.
4. Hand-kept lists (`cli_version_tests.rs`, `test.yaml` `check` and produced lists) give no error
   when forgotten. That is the standing gap (b).
