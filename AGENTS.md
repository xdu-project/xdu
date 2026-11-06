# AGENTS.md

Guidance for coding agents (Claude Code and others) working in this repository.
`CLAUDE.md` is a symlink to this file — edit `AGENTS.md`, never a separate copy. (Warp reads
`AGENTS.md` directly, so there is no separate `WARP.md`.) `.claude` is likewise a symlink to
`.agents`, so Claude Code discovers the factory skills and settings through it.

This document is the operating manual: the architecture, the load-bearing invariants, and the
process rules an autonomous agent needs to make correct, safe changes here without rediscovering
them each session. When something below disagrees with the code, **the code is ground truth — fix
this file.**

---

## Project

**xdu** is a high-performance filesystem indexer and query suite for large-scale storage
administration (HPC / enterprise: hundreds of millions to billions of files) where `du`(1) and
`find`(1) are too slow for regular auditing. You point it at a directory tree; it builds a
persistent, queryable **Parquet index partitioned by top-level subdirectory** once, then answers
size/age/pattern questions instantly. The directory names are bare, not Hive `key=value` pairs, so
DuckDB derives no partition column from them and partition scoping comes from the glob
(`lib::index_glob`) rather than from partition metadata.

Seven binaries (`Cargo.toml [[bin]]`), six user-facing plus one build helper:

| Binary | Role |
|--------|------|
| **`xdu`** | Crawler/indexer — walks a tree, writes the eight-column Parquet index (path, size, uid, gid, mode, atime, mtime, ctime). |
| **`xdu-find`** | Query CLI — DuckDB over the index; filters + `--count`/`--top`/formats. |
| **`xdu-view`** | ncdu-style interactive TUI (ratatui/crossterm); read-only list + tree views. |
| **`xdu-rm`** | **Destructive** bulk deletion of files matching an index query, with `--safe` re-stat. |
| **`xdu-cp`** | Bulk copy of the files matching an index query into a tree under `--into`, never replacing a file; `--safe` re-checks every filter. |
| **`xdu-mv`** | **Destructive at the source**: bulk move of the matching files into a tree under `--into` by a no-replace rename, or, to another filesystem, by a flushed copy and then the source's removal; `--safe` re-checks every filter. |
| **`gen-completions`** | Dev helper — emits bash+zsh completions from the `src/cli.rs` clap structs. |

The index schema is deliberately minimal: `path` (UTF-8), `size` (INT64 bytes), `uid`,
`gid` and `mode` (INT64 owner, group, and permission bits), `atime`, `mtime` and `ctime`
(INT64 Unix epoch seconds). It is **Unix-only** (`std::os::unix::fs::MetadataExt` for times,
ownership, and disk usage). Snappy compression. Queries use the **bundled** DuckDB (`duckdb` crate, `bundled`
feature) so there is no external DuckDB dependency.

## Environment & working rules

- **Commit only when explicitly asked.** When you do: this is **GitHub Flow on `main`** — branch
  off `main` (`feature/{slug}` or `fix/{slug}`), open a **squash** PR back to `main`. There is no
  `develop` branch; releases are `vX.Y.Z` tags on `main`.
- **Commit subjects follow `[category] Imperative summary`.** Common categories: `feature`, `fix`,
  `docs`, `ci`, `refactor`, `test`, `release` (version bumps / rebuilt assets), and `harness` (the
  `.agents/` factory). This set is **not closed** — coin a new lowercase category when one fits.
  Subject length, and the wrapping of the body, are stated once in § *Prose and comments*.
- **No `Co-Authored-By:` trailer on commits** — it is noise in `git log`. Authorship/AI-assistance
  is tracked in the **PR body** instead, which ends with an attribution trailer naming the
  actual harness, model, and variant.
- **Version is single-sourced from `Cargo.toml`** — never hardcode a version string in `src/`; read it
  from `CARGO_PKG_VERSION` (the completion marker does, `crawl.rs`). All six `#[command(...)]`
  blocks in `src/cli.rs` set `version`, so clap derives `-V`/`--version` from `Cargo.toml` for every
  user-facing binary.
- **A CLI change updates its `doc/*.scd` man page source in the same commit.** Shell completions
  regenerate automatically from `src/cli.rs` (`gen-completions`), so they are not committed; the
  generated `share/` tree is git-ignored (built in CI and by `/xdu-release`).
- **Commit-message hook** — `.githooks/commit-msg` reflows the body to ≤80 columns (subject,
  comments, and indented literal lines pass through untouched; a subject past 72 earns a warning,
  not a failure). Enable per clone with `git config core.hooksPath .githooks`. Write the body as
  flowing paragraphs; the hook wraps them.
- **Delete with `del`, never `rm`** — and read the exceptions in the next bullet before applying this
  to committed code. `del` ([`delete-cli`](https://pypi.org/project/delete-cli/), the maintainer's)
  moves a path to the `$HOME` trash instead of unlinking it, so a mistake is recoverable: `del PATH…`
  removes, `del --list` shows what is there, `del --restore PATH` undoes, `del --empty` reclaims. This
  binds **every** harness working in this repo, not just Claude Code. It is about the *act*, not the
  spelling — any irreversible removal of something you did not just create counts, whether written
  `rm`, `find -delete`, `git clean -fdx`, `shutil.rmtree` or `truncate`. Three sharp edges, each
  measured: **`-r` means `--restore`, not recursive** — `del -r dir` exits 0 and deletes *nothing*,
  while plain `del dir` handles directories natively; there is **no `-f`**, and while a missing path
  is exit 0, a *usage* error is not (`del -rf` exits 2), so do not assume success; and `del` needs a
  real `$HOME` — unset, it writes `.Trash/` and `.Trash.db` into the working directory, and under a
  redirected `$HOME` (as `tests/offline_tests.rs` and several `verify:` gates use) it lands in the
  fixture and reddens it. Not on `PATH`? In order: `uvx --from delete-cli del …` (ephemeral, installs
  nothing), else `uv tool install delete-cli` once. If neither resolves — no `uv`, no network — **stop
  and ask.** The interactive shell defines `rm` as a function that refuses, and routing around it with
  `command /bin/rm`, `\rm`, `env rm` or `sh -c 'rm …'` defeats a guardrail that exists on purpose.
  That bypass already happened here, and because the refusing `rm` **exits 0**, it read as a clean run
  while leaving `/usr/bin/false` installed as `target/release/xdu` for two later phases to measure.
- **…and the deletions that stay `rm`.** The rule above governs what an *agent* removes; it does not
  reach committed code, and converting these breaks things. Keep `rm` wherever there is no trash to
  move to, or nothing worth recovering: **container builds** (`Dockerfile` — the point of
  `rm -rf /var/lib/apt/lists/*` is layer size, and a move keeps the bytes in the layer); **the
  installer** (`install.sh` runs on a stranger's machine via `curl | sh`, is POSIX `sh` with no Python,
  and must not seed their trash); **CI**, which has no `del`; **HPC operator instructions**
  (`bench/HPC-PROTOCOL.md`) — there `$HOME` is a small quota'd filesystem separate from the scratch one,
  so a trash move becomes a cross-device copy of a multi-million-file index; **the benchmark trees**
  (`bench/`), which reach millions of sparse files, where trashing frees no disk and puts a rename
  inside the timed region; **self-cleaning scratch** — any `mktemp -d` that its creator disposes of,
  script or agent alike; **a tool's own bookkeeping** (`git rm`, `git worktree remove`, `cargo clean`,
  `docker prune`), which maintains metadata a trash move would strand; **published examples**
  (`README.md`, `doc/*.scd`) that teach users `xdu-find … | xargs rm`; and **`xdu-rm` itself**, whose
  entire product purpose is to free blocks.

## Prose and comments

The comments, documentation and commit messages here have a voice. Keep it. The padding, hedging and
marketing adjectives that generated text tends toward cost a reader's confidence in code that is
otherwise correct, and this is a tool that unlinks files on shared storage — it has to be taken
seriously to be trusted with that. `src/lib.rs` and `src/crawl.rs` carry the voice; match them.

**Write:**

- Declarative statements. `// Presence attests that the whole run finished; the body only adds
  detail.` — not `// This function will check...`.
- The **why**, not the what. The code says what it does. A comment earns its place by naming a
  constraint, a failure mode, or a rejected alternative. The `RESERVED_INDEX_NAMES` guard iterates
  the list so that reserving a new name rejects it by construction; that reason is not recoverable
  from reading the loop.
- Concrete failure modes. "`_OUTDIR_/*/*.parquet` published as `OUTDIR//.parquet`, at exit 0" beats
  "escaping matters here". "All 16 destructive tests drove a stale `target/release/xdu-rm` while
  `cargo test` stayed green" beats "the fixture was unreliable".
- Whole sentences with terminal punctuation, in the style already in the file.

**Do not write:**

- Filler and hedging: "simply", "just", "note that", "it's worth noting", "essentially",
  "basically".
- Marketing adjectives: "comprehensive", "robust", "seamless", "powerful", "elegant", "blazing",
  "leverage", "utilize". If a property matters, state the measurement — `bench/` exists so a claim
  about crawl speed carries a number, a tree shape, and a noise floor.
- "This ensures that…", "This allows us to…", "In order to…" — usually a sentence that has not yet
  decided what it is claiming.
- Restatements of the adjacent line. A comment that paraphrases the code is worse than none.
- Emoji, decorative Unicode, or exclamation marks in a comment, a `doc/*.scd` page, or a commit
  message. A Rust macro `!` is not prose. There is none in `src/` or `doc/` today, and the
  decoration the project keeps is confined to PR bodies (`🔧 Harness feedback`, the attribution
  trailer).
- Bulleted lists where two sentences would do. Tables are for reference material; prose is for
  reasoning.

**Never embed feature-scoped spec ids** (`R#`, `P#`) in `src/`, `doc/*.scd` or `README.md`. They
restart per feature, live in `spec/{slug}/`, and mean nothing in the merged tree; provenance runs
`git blame → commit → PR → spec/{slug}/`. Nothing lints this, so it survives on review alone. Naming
stable things is fine: real functions, real constants (`ROOT_PARTITION`, `MARKER_READ_LIMIT`), real
environment variables (`XDU_INDEX`, `XDU_JOBS`), documented DuckDB or `scdoc` behavior.

### Commit messages

The subject is wrapped by hand; the body is reflowed by the `commit-msg` hook. None of the numbers
below is a convention borrowed from elsewhere.

- **Subject: 72 characters, hard**, counting the `[category] ` prefix. `git log --oneline` spends 8
  columns on the short SHA and a space, leaving 72 of an 80-column terminal. This costs
  human-authored commits nothing: 2 of the 60 non-`[harness]` subjects on `main` exceed it, against
  a mean of 46.9.
- **Body: wrapped at 80 by the hook**, after a blank line. Write flowing paragraphs and let the
  hook reflow them. That is what this repo already writes — of 453 body
  lines, 3% exceed 80 where 43% exceed 72, so 72 would impose a new standard rather than record the
  one in force. `git log` indents the body 4 columns, so an 80-wide body can fold in an exactly-80
  terminal; that is the accepted cost of matching the corpus.
- **Body: 8-12 lines, and the *why* only.** The diff says what changed; the body says why, what was
  rejected, and what it cost. The corpus runs p50 = 1 line and p90 = 12 over the 143 commits
  preceding this rule, with two outliers at 21 and 29 — both redesigns reversing a committed
  decision, and both reasoning rather than narration, which is the shape that earns the length. A
  body past 12 usually means the commit wants splitting.
- **A commit that adds prose does not restate it.** When the change is a seed, a roadmap entry or a
  doc page, the reasoning is already in the file, and repeating it doubles the maintenance surface
  to say nothing new. The body earns its place only with what the file cannot say: why now, why
  here, what this replaces.
- **Exempt from the body wrap:** a URL, a pasted error string, or a path longer than 80. Breaking
  one makes it uncopyable, so give it its own line and let it overrun.

**When a subject will not fit, reword it — never truncate.** Drop the article, name the thing
instead of describing it, or split an "and also…" clause into a second commit. What overflows is
usually *provenance* rather than summary, and provenance belongs in the body:

```
[harness] Close the class by evaluating a predicate

Finding: manpage-literal-assertion-fails-on-ubuntu F9
```

That subject is 51 characters. Committed with `(manpage-literal-assertion-fails-on-ubuntu F9)`
appended it was 117 — the inline form that `.agents/skills/xdu-harness/SKILL.md` templates, where a
41-character slug spends 57 of the 72 columns before the summary begins. That template is the one
place the cap was structural rather than a matter of wording, and it is why 15 of the 19 `[harness]`
subjects on `main` exceed 72. Moving the suffix to a body line brings 18 of the 19 inside. The
holdout carries no suffix and is simply too long a summary, which is the case the cap exists for.

**Branch commits are out of scope.** The `xdu-feature|plan|build|review` subject templates repeat
the slug and can exceed 72, but `/xdu-publish` squashes them into the PR title and none reaches
`main`. The **PR title is in scope** — it becomes the squash subject, and GitHub appends ` (#N)` to
it, so leave room.

### Markdown prose

Hard-wrap markdown at **100 columns**, by hand. That is this repository's measured width rather than
an aspiration: `AGENTS.md` runs p50 = 96, p90 = 102. A rule of 80 would put a fifth of the
constitution in violation of a standard it states about itself.

**Count characters, not bytes.** `awk 'length > 100'` reports 95 long lines in this file where a
character count finds 69; the difference is em dashes, at three bytes each.

Tables are exempt — a row cannot be wrapped without breaking it, and the two widest lines here (167
and 141) are both rows. Fenced code, link targets, and any single token wider than the column are
exempt for the same reason. `doc/*.scd` is exempt as well: its breaks are chosen for roff, not for
reading, and where the two rules conflict the line-start restrictions in **Commands** win, because
rewrapping is the fix for a leading `.` and escaping is not.

Rewrapping a paragraph produces a whole-paragraph diff, so reflow only the paragraphs you were
editing anyway. The ~55 pre-existing prose lines in the 101–104 band are grandfathered, not a
backlog; do not open a diff to chase them.

## Commands

Toolchain is pinned by **`rust-toolchain.toml`** (edition 2024; the crate uses let-chains). Do not
assume `nightly` — use the pinned toolchain.

```bash
cargo build --release                                   # all bins (bundled DuckDB C++ is the slow part)
cargo test                                              # lib unit tests + tests/ integration
cargo test --test rm_tests                              # one integration file
XDU_TEST_EXTENSION_DIR=target/test-ext sh tests/s3-wire.sh "" 54   # S3 wire tier (see Testing)
XDU_TEST_XFS_DIR=/dev/shm/xdu-xfs sh tests/xfs-wire.sh "" 3   # cross-filesystem tier (see Testing)
cargo clippy --all-targets --all-features -- -D warnings   # lint gate (must be clean)
cargo fmt --all -- --check                              # format gate

# Drive the real binaries against a throwaway index (never a real one):
.agents/factory/bin/temp_index.sh xdu-find --count
.agents/factory/bin/temp_index.sh sh -c 'xdu-rm --dry-run -p "\.log$" --force'

# Man page gate. INSTALL scdoc — without it a doc/*.scd defect reaches CI unseen:
brew install scdoc                     # macOS  (apt-get install -y scdoc on Debian/Ubuntu, as CI does)
for scd in doc/*.scd; do scdoc < "$scd" > /dev/null || echo "FAILED $scd"; done
scdoc < doc/xdu.1.scd | mandoc -Tutf8 | col -b   # read the PUBLISHED text, not just exit 0

# ...and to ASSERT a literal survived, match it the way CI does. Strip all whitespace from the page
# AND from the literal: mandoc breaks lines wherever the fill lands, including inside a token, so an
# un-normalized grep calls an intact literal missing. Your scdoc is probably not CI's — homebrew's
# escapes hyphen-minus, the distro package does not — so the un-normalized form can pass for you and
# fail on ubuntu-24.04. Keep this in lockstep with the "Assert critical literals" step in
# .github/workflows/test.yaml and its restatement in .agents/factory/invariants.md §13.
lit='OUTDIR/.xdu-complete'
scdoc < doc/xdu.1.scd | mandoc -Tutf8 | col -b | tr -d '[:space:]' |
  grep -qF -- "$(printf '%s' "$lit" | tr -d '[:space:]')" || echo "MISSING: $lit"

# A literal that occurs MORE THAN ONCE needs a count, not a presence check — `grep -q` is satisfied
# by the surviving copy, so corrupting one of two occurrences reads green here and red in CI, which
# asserts it as `2x:.partial suffix`. Count the same normalized way. Keep the needle specific enough
# that stripping whitespace cannot FUSE two adjacent tokens into a synthetic match: `e.g. partial`
# becomes `e.g.partial`, so count `.partial suffix`, never a bare `.partial`.
lit='.partial suffix'; want=2
got=$(scdoc < doc/xdu.1.scd | mandoc -Tutf8 | col -b | tr -d '[:space:]' |
  grep -oF -- "$(printf '%s' "$lit" | tr -d '[:space:]')" | wc -l | tr -d ' ') || got=0
[ "$got" -eq "$want" ] || echo "MISCOUNT: $lit — $got, expected $want"
```

**Rendering is not optional, and exit 0 is not sufficient.** `scdoc` markup fails in two ways: a
nesting error is loud (`*__root__*` — `_` opens italic inside bold) and exits 1, but a mis-escaped
literal is **silent at exit 0** — `_OUTDIR_/*/*.parquet` published as `OUTDIR//.parquet` because `*`
is bold markup, and a line *starting* with `.` has that period silently dropped. Escape a literal
asterisk `\*` and a double underscore `\_\_` (mid-word `_` as in `*XDU_INDEX*` is safe); never start
a line with `.` or `'` — **rewrap, never escape**: `\.` at line start deletes the rest of the line,
also silently. None of this is mechanically fixable — `*/*` is a corrupted glob in `xdu.1.scd` and a
legitimate bold-slash (the `/` key) in `xdu-view.1.scd`, so intent has to be read. Catching the
silent class requires diffing the rendered text against the literal you intended, which is what the
`mandoc | col -b` line above is for.

**A third mode is not a markup error at all: the literal is intact and still unfindable.** `mandoc`
fills each paragraph to its own width and will break a line *inside* a token — a bare roff `-` is a
legal break opportunity — so `OUTDIR/.xdu-complete` can publish as `OUTDIR/.xdu-` + newline +
`complete`. Collapsing newlines to spaces does not recover it (the break was *within* a word, not
between two), and `col -b` indents continuation lines with a literal TAB, which squeezing spaces
never touches. That is **layout, not content**: nothing is corrupt, and the fix is to compare with
all whitespace stripped from both sides — the second snippet above. Whether you ever see it depends
on your `scdoc`: 1.11.5 substitutes `\-` for a bare `-` (upstream `1d4143d`), ubuntu-24.04's 1.11.2
does not, so the same `.scd` yields two different roffs and a check can be green on your box while
`main` is red. Which is exactly what happened — the gate arrived already broken and had never once
been green in CI. The normalization now lives in three places that must change together:
`.github/workflows/test.yaml`'s assertion step, the snippet above, and
[`invariants.md`](.agents/factory/invariants.md) §13.

**Presence is the wrong question for a literal that appears twice.** `grep -q` is satisfied by
whichever copy survived, so corrupting exactly one of `doc/xdu.1.scd`'s two `.partial suffix`
occurrences is invisible to it — which is why CI counts that one (`2x:.partial suffix`) instead. The
same is true of every `*XDU_INDEX*` / `*XDU_JOBS*` / `*XDU_EXTENSION_DIR*`: each appears twice on
its page, once as a cross-reference (in the flag description, or for `*XDU_EXTENSION_DIR*` in
the `S3 INDICES` section of `xdu-find.1` and `xdu-view.1`) and once as the `ENVIRONMENT` entry, so
corrupting only the `ENVIRONMENT` entry publishes a variable name that does not exist while a
presence check stays green.
All of them are counted for that reason, and so are the `AWS_*` names on the pages that list them
(`AWS_CA_BUNDLE` twice on each of `xdu.1`, `xdu-find.1` and `xdu-view.1`: the trust paragraph of the
S3 section, then its `ENVIRONMENT` entry) and `xdu.1`'s two `s3://BUCKET/PREFIX` (under `-o` and in
`S3 OUTPUT`). `xdu-cp.1` and `xdu-mv.1` add one more class, a **worked path**: each page's
`--root`/`--into` mapping example names a destination path (`/cold/alice/logs/app.log`,
`/archive/carol/runs/r1.h5`), counted once, so a mis-escaped layout example fails, and the two
pages use different paths, so each literal also says which page it is on. Keep such a path free of
`_`, `*` and `-`, and do not repeat it elsewhere on its page without re-counting.
**Whether a literal is duplicated is a fact about the page,
not about the literal** — adding one cross-reference to a `.scd` can duplicate a previously-unique
literal — so re-count when you edit a `.scd` that carries an asserted literal; nothing re-derives it
for you. A new page is not covered at all: the `check` list names six pages while the render step
globs `doc/*.scd`, so a seventh man page ships entirely unasserted. Both gaps are recorded in
[`issues/manpage-gate-coverage-gaps.md`](issues/manpage-gate-coverage-gaps.md).
A local presence check on a duplicated literal therefore *cannot* predict CI's verdict no matter how
correctly it normalizes; use the counting form above. Counting is what makes stripping whitespace
dangerous in the other direction: the strip is lossy and can fuse adjacent tokens into a match that
never appeared on the page, which is harmless for presence (at worst a false green) but a false
**red** for a count — so a counted needle must be specific enough that fusion cannot synthesize it.

## Repository map

```
src/
  lib.rs         # shared core: get_schema() (THE index schema), SizeMode, parse_size,
                 # SortMode, QueryFilters (DuckDB WHERE/ORDER BY builders), format_{count,bytes,speed},
                 # the index globs and parquet_source, and the readers' completion-marker read
                 # (local, and read_completion_marker_s3) with its version gate and warning
  cli.rs         # the SINGLE clap CLI definition (XduArgs/XduFindArgs/XduViewArgs/XduRmArgs, and
                 # SelectArgs flattened into XduCpArgs/XduMvArgs) — gen-completions + man pages
                 # describe exactly this
  select.rs      # the select-act engine's selection: S3 refusal, marker gates, the glob-metachar
                 # and partition refusals, ONE bound statement (read_parquet(?), ORDER BY path), and
                 # holds_live, the --safe re-check of every filter against live metadata
  atomic_fs.rs   # the filesystem primitives: O_NOFOLLOW component walks, open-only-regular-files,
                 # rename_noreplace (renameat2/renameatx_np, else a re-checked linkat), copy_atomic
                 # (.xdu-*.part temp, flush, no-replace publish), move_same_fs, move_across (the
                 # cross-filesystem copy, re-check, then unlink), the Faults/StepHook seam
  relocate.rs    # --root/--into mapping, whole-set refusals, prompt, the ordered executor on a
                 # local pool, outcomes and summary; run_cp/run_mv
  crawl.rs       # index-build hot path lifted out of the bin so it is unit-testable: work-queue
                 # construction (incl. the __root__ collision rejection), per-file record building,
                 # PartitionBuffer + IndexSink/LocalSink (atomic finalize), the local completion
                 # marker's clear/write. Only the crawler uses it; the readers' marker read and
                 # gates live in lib.rs. The concurrency scaffold stays in bin/xdu.rs.
  engine.rs      # the ONLY place a DuckDB connection is opened: open_connection() turns extension
                 # autoinstall + autoload off; PINNED_EXTENSIONS (the shipped set), extension_dir()
                 # (XDU_EXTENSION_DIR, else <canonical exe>/../../lib/xdu/extensions) and
                 # load_extension() (load by absolute path, or fail naming it; never download)
  s3.rs          # IndexLocator (classified on the OsStr), the ONE credential resolver (S3Settings)
                 # both directions use, and S3Sink: the crawler's S3 destination (object_store on
                 # ring, PUT/LIST/DELETE, partition-name refusal)
  view_query.rs  # xdu-view's index queries (partition list, __root__ files, partition root,
                 # directory listings) over a local or S3 index, out of the bin so tests reach them
  bin/xdu.rs     # indexer: shared-rayon-pool concurrent walk, driver threads, thread::scope error
                 # propagation, progress — the scaffold around lib::crawl
  bin/xdu-find.rs   # DuckDB query CLI
  bin/xdu-view.rs   # ratatui/crossterm TUI (read-only); its queries live in view_query.rs
  bin/xdu-rm.rs     # destructive bulk delete (safe mode, confirm, dry-run, parallel)
  bin/xdu-cp.rs     # bulk copy: thin, calls relocate::run_cp
  bin/xdu-mv.rs     # bulk move: thin, calls relocate::run_mv
  bin/gen-completions.rs   # emits share/ completions from cli.rs
tests/           # crawl_tests.rs, rm_tests.rs, cp_tests.rs, mv_tests.rs, offline_tests.rs
                 # (integration); common/ = shared helpers
                 # s3_tests.rs + s3-wire.sh: the S3 wire tier and its CI gate (see Testing);
                 # mv_xfs_tests.rs + xfs-wire.sh: the cross-filesystem tier and its CI gate;
                 # common/s3.rs = the loopback S3 endpoint, counting proxy, hermetic child env
examples/extension-probe.rs   # release-time probe, never installed: `manifest` prints engine version,
                 # platform, source_id + pinned set; `load` loads that set from the extension
                 # directory on one engine connection. Built in the same `cargo build` as the bins;
                 # its unit tests run under `cargo test` (`test = true` in Cargo.toml)
clippy.toml      # `disallowed-methods`: duckdb::Connection::{open,open_in_memory,*_with_flags} fail
                 # clippy -D warnings everywhere but src/engine.rs, so no connection skips the refusal
doc/*.scd        # scdoc man page SOURCES (authored); render to share/man/man1/*.1
share/           # GENERATED (man + completions); git-ignored; built in CI / by /xdu-release
build.sh install.sh Dockerfile   # packaging / ops scripts
packaging/extensions/   # footer.sh (check an extension's engine/platform footer), fetch.sh (download the
                 # probe's pinned set), check.sh (the release gate: runs every shipped binary and
                 # loads the set from the extracted tarball under a cold HOME, before upload),
                 # aws-tls-probe.sh (a smoke check that the tools verify AWS's real chain against
                 # the host's CA bundle; check.sh and the test legs run it)
packaging/rpm/   # xdu.spec: ad-hoc / COPR-style EL build from the source tag; no workflow runs it
                 # (see issues/native-os-packages-deb-rpm.md)
.agents/         # the spec-driven "software factory" (see below)
spec/{slug}/     # committed, dated per-feature design records the factory produces (retained on merge)
issues/{slug}.md # deferred code work, pre-shaped (status: unshaped); /xdu-feature promotes one to a GOAL
research/        # evidence produced BEFORE a cycle exists; /xdu-plan adopts a file as
                 # spec/{slug}/research/NN-*.md at promotion, and it is deleted from here then
ROADMAP.md       # forward-looking feature roadmap — prose intentions that seed future /xdu-feature
```

Two repo-level trees sit outside `src/`: **`.agents/`** — the software factory (the
`xdu-feature|plan|build|review|publish` lifecycle skills plus operational siblings `xdu-harness`
(meta/maintenance), `xdu-release` (version cuts), and `xdu-roadmap` (seed retirement); `factory/` methodology, invariants, EARS,
templates, the non-Claude `portability.md` contract, and the `bin/` FSM scripts + `meta_status.py`);
and **`spec/{slug}/`** — the `GOAL/PLAN/TECH/REVIEW.md` + `META.md` records the factory produces and
**retains on merge**. `AGENTS.md` stays ground truth; `spec/{slug}/` is a point-in-time record of
intent. See `.agents/factory/methodology.md`.

**Where a deferral goes — four homes, one rule each.** A pass that decides not to fix something must
record it, and the destination is not a matter of taste:

| File | Holds | Written by |
|---|---|---|
| `spec/{slug}/META.md` | **Harness/skill feedback only** — "was this the *factory's* fault". Never code follow-ups. | the lifecycle skills |
| `issues/{slug}.md` | **Deferred code work**, pre-shaped from [`templates/ISSUE.md`](.agents/factory/templates/ISSUE.md) with `status: unshaped` | whoever defers it |
| `ROADMAP.md` | the **ordered index** — one entry per issue, `**Seed:**` pointing at the `issues/` file | whoever defers it |
| `spec/{slug}/` | work **actually in flight** | the lifecycle skills |

An `issues/{slug}.md` is a *candidate*, not a contract: `/xdu-feature` promotes it into
`spec/{slug}/GOAL.md`, and that promotion is where appetite, non-goals and R-IDs get negotiated. Never
copy one into a `GOAL.md` verbatim — `xdu-review` grades a GOAL, and an unshaped proposal is not a
contract anyone agreed to.

This coexists with the **GitHub tracker** (`AGENTS.md` already cites issues #2/#3): a GH issue is the
public-facing ticket, `issues/{slug}.md` is the pre-shaped spec behind it, and the two may link to each
other. Neither replaces the other.

## Architecture & data flow

```
xdu ──walk (jwalk + shared rayon pool)──▶ PartitionBuffer ──.partial──▶ fs::rename ──▶ <index>/<part>/NNNNNN.parquet
                                    │                                                         │
                        on success  └──▶ <index>/.xdu-complete  (run-level attestation)        │
                                                     │                                         │
xdu-{find,view,rm,cp,mv}     ◀── refuse/warn by marker ┘  ◀── DuckDB read_parquet('<index>/<part-or-*>/*.parquet') ◀┘
```

An `s3://` index has the same layout under its prefix. `s3::S3Sink` PUTs each chunk at its final
key with no `.partial` stage, and `xdu-find` and `xdu-view` read the index in place through the
pinned `httpfs`, with the glob a bound parameter (`lib::parquet_source`); `xdu-rm`, `xdu-cp` and
`xdu-mv` refuse one.

- **Index layout:** `<outdir>/<partition>/NNNNNN.parquet`, where `partition` is a top-level
  subdirectory name. Loose files directly under the indexed root go to the reserved partition
  **`__root__`** (`ROOT_PARTITION`, `lib.rs`), crawled with `max_depth(1)`. Readers glob `*/*.parquet`.
  `<index>/` holds exactly two kinds of entry — partition directories and the marker dotfile below —
  so **both** their names are reserved (`RESERVED_INDEX_NAMES`, `lib.rs`, pairing each name with what
  claims it). A top-level source subdirectory of either name is **rejected** by
  `crawl::build_work_queue`, which iterates that list: it would clobber the synthetic partition's chunk
  ids, or occupy the marker path and brick the outdir for every later run.
- **Run-level completion marker:** `<outdir>/.xdu-complete` (`COMPLETION_MARKER`, `lib.rs`) — a
  dotfile, so the readers' `*/*.parquet` glob never mistakes it for a partition. Per-chunk
  `.partial`→rename is atomic for one *file* but cannot express whether the **run** finished: when one
  driver fails, partitions that already succeeded stay on disk as real `.parquet` chunks,
  indistinguishable from a complete index. So the marker is **cleared once pre-flight has passed** and
  the crawl is about to write, and **written only on the success path** — its presence attests to the
  whole run. Body is `key=value` lines (`xdu`, `completed_at`, `files`, `bytes`, `vanished`,
  `errors`, `lossy_paths`, `format`), so an `--allow-errors` run still records how much it skipped;
  `format` carries the run-level index format version (`INDEX_FORMAT_VERSION`, `lib.rs`). On a local
  index all three readers call `lib::index_version_error` before `lib::index_completion_warning`; on
  an `s3://` index `xdu-find` and `xdu-view` read the marker once with
  `lib::read_completion_marker_s3`, then call `lib::marker_version_error` before
  `lib::marker_completion_warning` on that read (the local pair are these two over the local read).
  Either way a marker stating no version they understand, absent marker included, is a refusal — no
  rows, no deletions — and a versioned marker recording tolerated errors still only earns a **soft
  stderr warning**. Reads are capped (`MARKER_READ_LIMIT`) and non-blocking, so a FIFO or huge file
  or object left at that path cannot hang or exhaust a reader. Locally that is one `stat` before any
  open. Over S3 it is a LIST proving the prefix holds objects, then a HEAD for the marker's size (a
  missing key is an absent marker), and a marker over the cap is refused as
  `MarkerRead::Oversized` without its GET. **Known limitation:** the marker
  describes the whole index but its counts come from one run, so a `--partition`-scoped run rewrites it
  from its own stats — see `issues/marker-scoped-run-attestation.md`.
- **Concurrency (indexer):** a **single** rayon pool (`Parallelism::RayonExistingPool`) backs **all**
  jwalk walkers; up to `--jobs` driver `std::thread`s pull partitions from a `Mutex<VecDeque>` work
  queue; rayon work-stealing balances directory reads across all active walkers so one huge partition
  can't starve the rest. `std::thread::scope` joins the drivers; the first `Err`/panic fails the run.
  Thread budget: N pool + C drivers + 1 main, plus, for an `s3://` output, the 2 worker threads of
  `S3Sink`'s private tokio runtime, which driver threads `block_on` and rayon workers never touch.
- **Atomic writes:** every write goes through `crawl::IndexSink`. Locally (`LocalSink`) each chunk
  is written to `NNNNNN.parquet.partial`, then `fs::rename`d to `.parquet` in `finalize()` (atomic
  within a dir on POSIX); stale higher-numbered chunks from a prior larger run are pruned up to the
  first gap. Into S3 (`s3::S3Sink`) each chunk is **one PUT at its final key** as it is encoded (a
  single PUT is all-or-nothing, and nothing is staged on local disk), and finalize is a LIST of the
  partition and a DELETE of **every** higher-numbered chunk, past gaps. Either way this is
  **per-file** atomic, **not** per-partition atomic — see invariants.
- **Query:** `xdu-find`/`xdu-view`/`xdu-rm` open an in-memory DuckDB, glob the index, and apply a
  `QueryFilters` WHERE clause. `xdu-view` is a read-only TUI; `xdu-rm` unlinks the matched files.
  `xdu-cp` and `xdu-mv` select through `select::select` (one statement, the glob bound, `ORDER BY
  path`) and act through `relocate`: each file goes to `<--into>/<path relative to --root>`, by
  `atomic_fs::copy_atomic` or a no-replace rename, never replacing an existing name.
  For an `s3://` index, `xdu-find` and `xdu-view` open the connection with
  `engine::open_s3_connection` (the pinned `httpfs` and one bound `CREATE SECRET`), pass the marker
  gates above, and take their `FROM` from `lib::parquet_source`.

## CLI surface (`src/cli.rs` is the one definition)

- `xdu DIR -o/--outdir DIR|s3://BUCKET/PREFIX [-j/--jobs N (env XDU_JOBS, dflt 4)] [-B/--buffsize N (100000)]
  [--apparent-size] [-k/--block-size SIZE] [-p/--partition NAMES] [--allow-errors]`
  — `--allow-errors` is **opt-in**: by default any walk/stat error fails the run non-zero and no
  completion marker is written; with it, unreadable entries are counted, reported on stderr, the run
  exits 0, and the marker records the tolerated `errors=N`.
- `xdu-find -i/--index DIR|s3://BUCKET/PREFIX (env XDU_INDEX) [-p/--pattern PATTERN] [--regex] [-u/--partition NAME]
  [--min-size/--max-size SIZE] [--older-than/--newer-than DAYS] [-f/--format path|size|atime|csv|json]
  [-l/--limit N] [-c/--count] [--top N]`
- `xdu-view -i/--index DIR|s3://BUCKET/PREFIX (env XDU_INDEX) [-u/--partition NAME] [-p/--pattern PATTERN] [--regex] [--min/max-size]
  [--older/newer-than] [-s/--sort name|size-asc|size-desc|count-asc|count-desc|age-asc|age-desc]`
- `xdu-rm -i/--index DIR [-p/--pattern PATTERN] [--regex] [-u/--partition] [--min/max-size]
  [--older/newer-than] [-l/--limit N] [-n/--dry-run] [--safe] [-f/--force] [-v/--verbose] [-j/--jobs (env XDU_JOBS)]`
  — `xdu-rm` takes a local index only and refuses an `s3://` one, from `-i` or `XDU_INDEX`.
- `xdu-cp` / `xdu-mv` `-i/--index DIR (env XDU_INDEX) --into DIR [--root DIR (dflt /)] [-p/--pattern
  PATTERN] [--regex] [-u/--partition NAME] [--min/max-size] [--older/newer-than] [--owner USER]
  [--group GROUP] [--mode SPEC] [--mtime-older/newer-than] [-l/--limit N] [-n/--dry-run] [--safe]
  [-f/--force] [-v/--verbose] [-j/--jobs (env XDU_JOBS)]` — every `xdu-find` filter plus `xdu-rm`'s
  action flags (one `SelectArgs`); local index only, `s3://` refused. `-f` is `--force` here, not
  `xdu-find`'s `--format`.
- An `s3://` locator (lowercase scheme only) takes its credentials, region and endpoint from the
  `AWS_*` variables and the shared credentials and config files, through `s3::S3Settings::resolve`,
  the one resolver both directions use. The readers need the pinned `httpfs` from
  `lib/xdu/extensions/` (or `XDU_EXTENSION_DIR`); the writer needs no extension.

**Footgun:** `-p` means `--partition` in `xdu` but `--pattern` in the three query tools (where
partition moves to `-u`). Same short flag, different meaning per binary — do not "fix" one side in
isolation.

## Load-bearing invariants (footguns)

The curated, numbered gate is [`.agents/factory/invariants.md`](.agents/factory/invariants.md),
kept **in lockstep** with this section (this file wins if they drift). The `xdu-plan` gate and the
`xdu-review` footgun checklist both draw from it. Summary of what must not silently break:

1. **Parquet schema stability.** `lib.rs::get_schema()` is the ONE contract: exactly eight
   **non-null** fields in fixed order — `path: Utf8`, `size: Int64`, `uid: Int64`,
   `gid: Int64`, `mode: Int64`, `atime: Int64`, `mtime: Int64`, `ctime: Int64`. Every reader
   selects these by name. The on-disk version is the `format` key in the `.xdu-complete` marker
   (`INDEX_FORMAT_VERSION`, `lib.rs`): version 2 names this eight-column layout, and every reader
   refuses a marker whose version it does not understand instead of misreading the rows — so any
   change to `get_schema()` or a reader's column list is a breaking, cross-cutting index-format
   change that must bump the version in the same commit (issues #2/#3 — owner/group/perms — are
   exactly this).
2. **Atomic finalization.** Write `NNNNNN.parquet.partial` → `fs::rename` (same dir) → prune stale
   higher chunks (`crawl.rs::LocalSink`, driven by `PartitionBuffer::finalize()`). Never
   `File::create` a final `.parquet`, never cross-dir rename, never let a reader glob `.partial`. An
   `s3://` output (`s3.rs::S3Sink`) has no rename: each chunk is one PUT at its final key, and
   finalize is a LIST plus a DELETE of every higher id, past gaps. **Known limitation:** finalize is
   per-file, not per-partition atomic — do not index a partition while purging it. Run-level
   completeness is a *separate* mechanism: the `.xdu-complete` marker (see Architecture), cleared after
   pre-flight and written only on success. Never write it on a failure path. Over S3 the pre-flight
   ends with one LIST of the prefix and every partition-name refusal, so the marker DELETE is the
   run's first write.
3. **Partition scheme.** `<index>/<partition>/NNNNNN.parquet`; `__root__` reserved for loose
   top-level files (depth-1 walk); zero-padded sequential chunk ids; readers glob `*/*.parquet`. The
   index root's namespace is a **class, not two facts**: it holds partition directories plus the
   `COMPLETION_MARKER` dotfile, and *every* name in `lib::RESERVED_INDEX_NAMES` is **rejected** as a
   top-level source directory by `crawl::build_work_queue`, unconditionally — even when a `--partition`
   filter would exclude it, because the collision is with the layout, not with the selection. Reserving
   a new name at the index root means adding it to that list in the same commit; the guard iterates the
   list, so the rejection follows by construction.
4. **`xdu-rm` destructive safety.** Default requires interactive `y/N` (anything but `y`/`yes`
   aborts); `--dry-run` deletes nothing; `--force` skips the prompt; `--safe` re-stats each file
   immediately before unlink. **Any deletion combined with `--limit` MUST carry a deterministic
   `ORDER BY`** so `--dry-run` and the real run select identical rows. `xdu-cp` and `xdu-mv` carry
   the same gates through the engine, with `ORDER BY path` always and a `--safe` that re-checks
   every filter; `xdu-mv` removes the source name, so it is destructive at the source.
5. **DuckDB injection surface.** Every user value reaching `read_parquet(...)`/`WHERE` is an
   injection surface. Route it through one validated escaping/quoting helper (or bound parameters);
   never raw `format!` a partition name or index path into SQL. For an `s3://` index the readers
   go through `lib::parquet_source`, which binds the whole glob (bucket, prefix and any `-u`
   partition) as the statement's one parameter, and the marker read binds its URLs too; a local
   index keeps the raw-`format!` glob, a known exception filed as a follow-up. The select-act engine
   (`select::selection_sql`, used by `xdu-cp` and `xdu-mv`) has no such exception: its one
   statement binds the local glob (`read_parquet(?)`) and the regex, and `select::checked_glob`
   refuses, before any SQL, an index path holding a glob metacharacter and a partition name that is
   empty, `.` or `..`, or holds `/` or a metacharacter. Credentials reach
   DuckDB only as the bound values of `engine::open_s3_connection`'s one prepared `CREATE SECRET`,
   never as SQL text. The extension path is one too:
   `engine::load_extension_from` puts the extension file's absolute path, from `XDU_EXTENSION_DIR`
   or the canonical exe's `../../lib/xdu/extensions`, into `LOAD '<path>'`, and refuses a path
   containing `'` or not valid UTF-8 before any SQL runs. The unit tests
   `engine::tests::quoted_directory_is_refused_before_any_sql` and
   `engine::tests::non_utf8_directory_is_refused_before_any_sql` hold that rule. **Every S3
   connection authenticates the server against one trust source**, which `S3Settings::resolve`
   picks once for both directions: the file `AWS_CA_BUNDLE` names, else the first of
   `s3::PLATFORM_CA_BUNDLES` present; a missing, unreadable or certificate-free source, or one
   holding no self-signed root, refuses before any request, naming `AWS_CA_BUNDLE`. The writer's client trusts only that file's
   certificates (`tls_certs_only`). The reader's `engine::open_s3_connection` runs fixed text
   after the `LOAD`, pinning `httpfs_client_implementation` to `httplib`, turning
   `enable_server_cert_verification` and `enable_curl_server_cert_verification` on, and binding the
   path as `SET ca_cert_file = ?`, never as SQL text. Nothing disables verification, nothing adds
   roots beside the source, and `SSL_CERT_FILE`, `SSL_CERT_DIR`, OpenSSL's compiled-in directory
   and the macOS keychain are never sources; a plain `http://` override resolves none. The `tls_`
   wire tests hold it (`invariants.md` §5). One known exception: the readers' host-name check is
   httpfs's httplib, which accepts a certificate whose CN names the host when its SAN does not;
   the writer refuses it. It is recorded until the follow-up fix cycle seeded as
   `issues/s3-reader-tls-hostname.md` lands, and nothing else in the rule has one.
6. **Unix-only.** `MetadataExt` (atime; disk usage = `st_blocks`×512) is load-bearing; don't add
   code assuming portability. A future S3/Windows source must special-case (S3 has no atime).
7. **Shared rayon-pool concurrency.** See Architecture. Preserve the single-pool + driver-thread +
   work-stealing model and `thread::scope` error propagation.
8. **Symlinks excluded.** `follow_links(false)` + `is_file()` — the index holds only regular files.
9. **`SortMode` age inversion.** `age-desc` = oldest first = `atime ASC` (and vice versa); the SQL
   (`to_order_by`/`to_partition_order_by`) and `xdu-view`'s in-memory sort must agree.
10. **CLI single source of truth.** `src/cli.rs` is the one definition; completions and `doc/*.scd`
    describe it; a CLI change updates `doc/*.scd` in the same commit. Prefer clap `ValueEnum`/
    `value_parser` over late string validation.
11. **Altitude / testability.** Bins stay thin (arg parse, terminal setup/teardown, event loop);
    parsing/formatting/sniffing/SQL-building/state-transition logic lives in `lib` so tests reach it.
12. **TUI terminal safety.** Raw mode + alternate screen restore on **every** exit path including
    panic (Drop guard / panic hook, not sequential code — `panic="abort"` is set); previewed file
    bytes are `strip_ansi`-sanitized before rendering.
13. **Project conventions.** Version single-sourced from `Cargo.toml`; `share/` generated + ignored +
    CI-asserted; tarball layout (`bin/` + `share/{man,bash-completion,zsh}` +
    `lib/xdu/extensions/`) matches `install.sh`;
    non-TTY runs keep stdout clean/pipeable (progress → stderr); reuse shared helpers; **no `spec/`
    R#/P# ids in source** (they restart per feature and collide across branches); prose, comment and
    commit-message voice per § *Prose and comments*, which is hygiene rather than a review finding.

## High-risk files & footguns (quick reference)

- **`src/bin/xdu-rm.rs`** — destructive. `--limit` needs a deterministic `ORDER BY`; `--safe` must
  re-verify the criteria it claims to (atime/size today; min-size/newer-than/pattern are gaps); the
  confirm/dry-run/force gates are load-bearing.
- **`src/select.rs`, `src/atomic_fs.rs`, `src/relocate.rs`, `src/bin/xdu-mv.rs`** — the select-act
  engine behind `xdu-cp` and `xdu-mv`. The selection is one bound statement with `ORDER BY path`;
  `--safe` must re-check every filter (`select::holds_live`); a final name is only ever published by
  `atomic_fs::rename_noreplace`, so a replacing rename or copy (`fs::rename`, `fs::copy`) or a
  truncating open (`File::create`, `O_TRUNC`) there breaks the never-replace rule; and `xdu-mv`
  removes source names, so it is destructive at the source. Across filesystems
  (`atomic_fs::move_across`, taken only when the rename answers `EXDEV`) the source is removed
  only after the copy and its directory are flushed and the source name re-checked against the
  opened file (dev, ino, type, size, mtime, ctime); any earlier exit withdraws the copy, so a file
  is never at neither name. Never write the source's times there: that moves its ctime.
- **`src/lib.rs`** — `get_schema()` (schema stability), `QueryFilters` (the reader-query injection surface),
  `index_glob` and `index_glob_s3` (which share the one tail that writes the index layout),
  `parquet_source` (which puts that glob into `xdu-find`'s and `xdu-view`'s SQL, as text locally
  and as a bound parameter for S3), the marker gates of `invariants.md` §2b on both transports
  (`index_version_error`, `marker_version_error`, `read_completion_marker_s3`), and the
  `ROOT_PARTITION` / `COMPLETION_MARKER` layout constants with the `RESERVED_INDEX_NAMES` list
  that guards both.
- **`src/engine.rs`** — the one place a DuckDB connection opens (autoload and autoinstall off), and
  `load_extension_from`, whose `LOAD '<path>'` is the extension-path injection surface (invariant 5);
  `open_s3_connection` adds the reader's one bound `CREATE SECRET`, and before it the fixed statements
  that pin httplib, turn certificate verification on and bind the trust source's path as the CA file.
- **`src/s3.rs`** — the S3 side of invariants 2 and 2b (`S3Sink`: one PUT per chunk, the LIST and
  DELETE prune past gaps, the marker DELETE first and PUT last), the S3 partition-name refusal, and the
  one credential resolver. Its `CREATE SECRET` must bind every key `httpfs` would otherwise take from
  the environment, and no request error may print a response body (one can echo the session token).
  The resolver also picks the one TLS trust source (`AWS_CA_BUNDLE`, else the first of
  `PLATFORM_CA_BUNDLES`) for both directions, refusing one that holds no self-signed root, and
  `S3Sink`'s client trusts only its certificates (`tls_certs_only`); a change here can widen or
  drop server authentication (invariant 5).
- **`src/view_query.rs`** — every `xdu-view` statement, on the invariant 5 surface.
- **`src/crawl.rs`** — `PartitionBuffer::finalize()` atomicity + stale-chunk pruning (through
  `IndexSink`; `LocalSink` is the local protocol); the reserved-index-name collision rejection in
  `build_work_queue`; completion-marker write/clear ordering.
- **`src/bin/xdu.rs`** — the shared-pool concurrency model (driver threads, `thread::scope` error
  propagation); marker clear-after-pre-flight / write-on-success sequencing.
- **`src/bin/xdu-view.rs`** — terminal restore on panic; multibyte-safe truncation; empty-list
  bounds; unbounded preview memory.

## Testing

- `lib.rs` carries strong pure-function unit tests (parse_size, SizeMode, SortMode, QueryFilters,
  formatters, get_schema). Keep new logic in `lib` so it is testable.
- Integration tests in `tests/` drive the **real binaries** (`std::process::Command`) against real
  temp indexes (`tempfile` + `libc` dev-deps). **`tests/common/mod.rs` is the one home for binary
  resolution and fixtures** — every `tests/*.rs` carries `mod common;` and takes `binary_path` (which
  is `CARGO_BIN_EXE_*`, so a test always drives the artifact Cargo built for the profile under test),
  `build_index`, the `run_*` wrappers, and `set_atime_days_ago` (backdates atimes via `utimensat`)
  from it. A fixture that lives in one test file instead is how `rm_tests.rs` came to re-declare a
  `binary_path` that preferred whatever was lying in `target/release/`, so all 16 destructive tests
  ran a stale binary while `cargo test` stayed green. Never reimplement production logic in a test.
  Some cases self-skip where the platform can't host them (running as root; APFS rejecting non-UTF-8
  filenames) — a skipping test still prints `ok`, so check with `--nocapture` before trusting a green
  suite to mean a case actually ran.
- **`offline_tests.rs` guards air-gapped operation.** The readers must answer a query with an empty
  DuckDB extension cache and write nothing into it; the test redirects `HOME` and asserts that
  directory stays empty. That now holds by refusal: every connection opens through
  `engine::open_connection`, which turns extension autoload and autoinstall off, so a query that
  needs an unlinked extension fails instead of downloading one. `Cargo.toml` still asks `duckdb` for
  `parquet` alongside `bundled`, because `bundled` alone leaves the Parquet reader unlinked, and with
  autoload refused that is a reader that cannot read its own index. Any other extension ships as a
  file in `lib/xdu/extensions/` and loads by path (`XDU_EXTENSION_DIR` overrides the directory);
  nothing is ever fetched at runtime.
- **The S3 wire tier is a standing CI gate that plain `cargo test` skips.** `tests/s3_tests.rs`
  holds two tiers. A few tests need only the binaries and run in every `cargo test`; the rest talk
  to an in-process `s3s` endpoint on loopback through the pinned `httpfs`, so they are `#[ignore]`d
  and plain `cargo test` lists them as ignored. To run them, fetch the pinned `httpfs` once into a
  directory the binaries never look in by default — **never** `target/lib/xdu/extensions`, which
  would hide a reader that wrongly loads it for a local index — and run the gate script:
  `cargo build --locked --example extension-probe`, then
  `sh packaging/extensions/fetch.sh target/debug/examples/extension-probe target/test-ext`, then
  `XDU_TEST_EXTENSION_DIR=target/test-ext sh tests/s3-wire.sh "" N`. The script passes only when
  every test `-- --list --include-ignored` lists also passed and that count is at least `N`, since
  under `--include-ignored` libtest always prints `0 ignored`. CI's `Tests` workflow fetches the
  extension and runs the script on both test legs as the "S3 tests" step, with `N` set to the file's
  listed count by hand: **raise it in the commit that adds a test**. A selection by prefix
  (`harness_`, `find_`, `write_`, `view_`, `tls_`) runs one family. The `tls_` family reaches the
  endpoint over `https` through `TestS3::tls_front`, a TLS terminator before the counting proxy,
  with certificates `TestPki` makes per run (a test CA, an unrelated CA, a wrong-name leaf, a
  self-signed end-entity one, an intermediate CA with a leaf it signed, and two leaves whose CN
  alone names the host), and asserts on proxied requests, never on handshakes. Wire tests never return early (the script
  refuses an early `return`); the fixture panics when it cannot bind or find the extension. Spawned
  binaries run hermetic (`tests/common/s3.rs`'s `Hermetic`: no `AWS_*`, `DUCKDB_*`, `*_PROXY`,
  `SSL_CERT_FILE` or `SSL_CERT_DIR`, empty `HOME`, `TMPDIR` and working directory, checked empty
  afterwards).
- **The cross-filesystem tier is a standing CI gate that plain `cargo test` skips.**
  `xdu-mv` moves a file to another filesystem by copying it and removing the source, and only a
  real second filesystem makes a rename answer `EXDEV`. `tests/mv_xfs_tests.rs` drives the binary
  there; every test is `#[ignore]`d and needs `XDU_TEST_XFS_DIR`, a writable directory on a
  filesystem other than the temp dirs'. The fixture panics with "XDU_TEST_XFS_DIR is not set"
  when it is unset and with "XDU_TEST_XFS_DIR is not on another filesystem" unless a probe rename
  from a temp dir into it fails with `EXDEV`, so the tier cannot pass on one filesystem. Run it
  with `XDU_TEST_XFS_DIR=DIR sh tests/xfs-wire.sh "" N`, which passes only when every listed test
  passed and that count is at least `N`. On Linux, `/dev/shm` is a tmpfs any user can write
  (`mkdir -p /dev/shm/xdu-xfs`). On macOS, attach an APFS disk image:
  `hdiutil create -size 256m -fs APFS -volname xdu-xfs IMG.dmg`, then
  `hdiutil attach -nobrowse -mountpoint DIR IMG.dmg` (an agent sandbox refuses both). CI's
  `Tests` workflow makes that directory on both test legs, checks its device differs from the
  workspace's, and runs the script as the "XFS tests" step, with `N` the file's listed count:
  **raise it in the commit that adds a test**. The fault-seam unit tests of
  `atomic_fs::move_across` (fail and abandon at every step, the flush order, a changed or swapped
  source, a refused unlink) run in every `cargo test` on every host.
- **Verify by driving the CLI, not just tests.** Use `.agents/factory/bin/temp_index.sh` so a drive
  hits a throwaway index, never a real one. Exit 0 is necessary but not sufficient — assert a concrete
  post-condition (row count, a stdout token, files actually gone/kept).
- **Performance work is measured, not asserted.** `bench/` holds the crawl benchmark: `gen_tree.py`
  (sparse synthetic trees — full `stat` cost, ~0 disk), `run.sh` (the runner; emits one JSON document
  per invocation), `scenarios.md` (the shape table + comparability rules), `HPC-PROTOCOL.md` (the
  protocol community operators run on real Lustre/GPFS/ZFS), and the committed
  `results/baseline.json` reference. `sh bench/run.sh smoke` is the fast non-rot check and asserts the
  index holds exactly the generated files. **`bench/spikes/` is not part of that**: it holds
  throwaway probes kept only because their answers became commitments, each traceable to a claim in
  `research/`. They sit outside `tests/` because they need a container and a network and several
  print failures rather than asserting them; `bench/spikes/run.sh` copies one in for a single run.
  Retire the directory with the cycle it informed. **Comparing two builds requires an interleaved A/B in a
  single invocation** — `run.sh --compare-bin PATH` (build the older side in a `git worktree`, never
  a stash). Two separate invocations cannot resolve it: on the reference host the same binary drifts
  up to ~20% between invocations, and >2× on the flat-wide shape, so a two-document comparison
  measures the session, not the code. See `bench/scenarios.md` "The noise floor".

## Packaging & release

- `share/` is generated: man pages via `scdoc` from `doc/*.scd`; completions via `gen-completions`
  from `src/cli.rs`. The `.scd` sources carry **no version string**, so a pure version bump does not
  touch them — they change only when the CLI changes.
- Release profile (`Cargo.toml`): `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
- Release tarball layout is a contract: `bin/{xdu,xdu-find,xdu-view,xdu-rm,xdu-cp,xdu-mv}` +
  `share/{man/man1/*.1, bash-completion/completions/*, zsh/site-functions/*}` +
  `lib/xdu/extensions/{name}.duckdb_extension` (the pinned DuckDB extensions, one per entry, built
  for that tarball's engine version and platform; flat, no version or platform directories).
  `install.sh`'s extraction mirrors it exactly, placing `lib/xdu/extensions/` beside the physical
  directory the binaries went to, since the tools look in `<canonical exe>/../../lib/xdu/extensions`
  unless `XDU_EXTENSION_DIR` names another directory. The container image and the HPCCM recipe keep
  the same layout under their prefix. Every release leg runs `packaging/extensions/check.sh` on the
  exact tarball it uploads, under a cold `HOME`, before the upload; it executes every shipped binary
  and loads the pinned set from the extracted layout, and runs `aws-tls-probe.sh`, which shows the
  shipped tools find the platform's CA bundle and verify AWS's real chain with it (any HTTP answer
  to AWS's documented example key passes; a platform smoke check, not acceptance). `cargo install` builds carry no
  extensions. `/xdu-release` cuts versions (bump `Cargo.toml` + `Cargo.lock`, gate, tag, publish);
  see its skill.
- **Pre-release gate (mirrored by CI and `/xdu-release`):** `cargo fmt --all -- --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` must all pass clean.
  `cargo test` skips the S3 wire tier; CI runs it through `tests/s3-wire.sh` (see Testing).

## Working on this codebase as an agent

- **Use the factory for non-trivial work.** A feature/fix/refactor flows through the `.agents/`
  spec-driven lifecycle — `/xdu-feature` (shape `GOAL.md`) → `/xdu-plan` (research + `PLAN.md`/
  `TECH.md`) → `/xdu-build` (execute phases) → `/xdu-review` (blind, evidence-based QA) →
  `/xdu-publish` (squash PR to `main`), each on a `feature/`|`fix/` branch with artifacts under
  `spec/{slug}/`. `.agents/factory/methodology.md` is the *why*; `.agents/factory/invariants.md` is
  the curated footgun checklist derived from this file. **Ceremony scales to appetite** — a
  one-sentence change may skip the lifecycle entirely. Each lifecycle skill ends with a
  silence-by-default meta-note to `spec/{slug}/META.md`; `/xdu-publish` surfaces substantial notes in
  the PR; the human-gated **`/xdu-harness`** applies them back to `.agents/` (logging
  `factory/harness-log.md`) and **never weakens a non-negotiable gate** on a finding's say-so.
- **Cut releases with `/xdu-release`** (operational, not a lifecycle step): bump `Cargo.toml`, run the
  CI-mirror gate, sign a tag, publish — rehearsed in a `git worktree` dry-run and gated on an explicit
  human OK before any irreversible push.
- **Put logic where it belongs:** shared types/parsing/SQL-building in `lib`; keep bins thin. Don't
  duplicate a helper across bins (the crawl-test reimplementation is the anti-pattern).
- **Prose, comment and commit-message voice** per § *Prose and comments*, including the ban on
  feature-scoped spec ids in source.
- **This file is the map, but it drifts.** For a deep change, re-verify the specific invariant against
  the source before relying on it, and update this file when the code moves.
