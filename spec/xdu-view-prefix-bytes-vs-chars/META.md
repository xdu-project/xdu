# META — `xdu-view` lists full child names under a non-ASCII directory

> **Harness feedback log** for this feature — the producer artifact of the factory's self-improvement
> loop. Written by the lifecycle skills (`xdu-feature` / `xdu-plan` / `xdu-build` / `xdu-review`) when the
> **skillset itself** costs something; read by `xdu-publish` (surfaced in the PR) and applied by
> `/xdu-harness`. This file is **orthogonal** to the `GOAL → PLAN → TECH → REVIEW` spine — it is about
> the *toolchain*, not the feature — and is retained on merge like the rest of `spec/xdu-view-prefix-bytes-vs-chars/`.
>
> **Silence is the default.** The bar for a finding is one test: *was this the **skill's** fault — not
> mine, not the task's?* A merely-hard task, a self-inflicted error, or a one-off content/code issue
> (that belongs in `GOAL.md` / `REVIEW.md`) is **not** a finding. The blind `xdu-review` correctness
> reviewer never reads this file — it would leak author intent.

- **slug:** xdu-view-prefix-bytes-vs-chars

## Friction findings

Field enums — `severity`: `high` (a safety / gate / correctness gap) `| medium | low`; `category`:
`instruction | steering | tooling | template | missing-guidance`; `status`: `open` (written by skills)
`| applied | rejected | deferred` (written by `/xdu-harness`).

Schema (copy one block per finding, appending it **after** this fence — the fence is illustrative and
is skipped by the parser):

```markdown
## F1 — <one-line title of the skillset problem>
`origin=<skill>:<step> severity=<high|medium|low> category=<instruction|steering|tooling|template|missing-guidance> status=open target=<best-guess file>`
- **What happened:** <what the skill made you do, or fail to do>.
- **Skill cause:** <why this is the instructions' fault — not yours, not the task's>.
- **Recommended fix:** <the concrete change to the skill / template / script>.
- **Confidence:** <high|med|low> · **Effort:** <small|medium|large>
```

<!-- Real findings are appended below this line by the lifecycle skills. -->

## F1 — No step commits a seed that planning itself files
`origin=xdu-plan:8 severity=low category=missing-guidance status=open target=.agents/skills/xdu-plan/SKILL.md`
- **What happened:** the GOAL's ruling required filing an `issues/` seed and a `ROADMAP.md` entry at plan time if a check reproduced; it did, but Step 8 stages only `spec/{slug}`, so the seed went in a separate commit whose category and timing the skill does not name.
- **Skill cause:** Step 6 covers deferrals named in phase bodies (recorded by the build's last phase), not a deferral the plan's own checks surface; the commit step assumes planning writes nothing outside `spec/{slug}`.
- **Recommended fix:** add to Step 8: a seed filed during planning lands in its own `[docs]` commit before the plan commit, and PLAN's rabbit-holes section links it.
- **Confidence:** med · **Effort:** small

## F2 — No helper for "fails on `main`, passes with the fix"
`origin=xdu-plan:6 severity=medium category=missing-guidance status=open target=.agents/factory/bin/`
- **What happened:** GOAL R5 requires the regression test to fail on the base commit; a green-only `verify:` cannot show that, so the plan wrote `spec/{slug}/verify/red.sh` (extract the base, or the fix with one mutated line, with `git archive`, overlay `tests/`, build into a separate target dir, require the named test to fail on a named assertion).
- **Skill cause:** fix cycles routinely carry a "fails on main" criterion, and Step 6's `verify:` guidance only describes green gates; each plan has to reinvent the red check and its pitfalls (shared `target/` swaps `CARGO_BIN_EXE_*` binaries; a compile error looks like red).
- **Recommended fix:** promote a generic `red_on_base.sh BASE TEST TOKEN -- CARGO_ARGS` into `.agents/factory/bin/` and name it in Step 6 for `kind: fix`.
- **Confidence:** med · **Effort:** small

## F3 — A red gate that shares one target dir can test the wrong tree · seen again (xdu-review:step2)
`origin=xdu-build:P2 severity=high category=tooling status=open target=.agents/factory/bin/ (the F2 helper)`
- **What happened:** `verify/red.sh` builds every red tree into one `target/red-target`. Cargo
  keys a path package by paths relative to its root, so every tree gets the same hashes. `git
  archive` keeps commit-time mtimes, so the `base` tree looked fresh and reused the previous
  mutant's `xdu` library. The base red passed the gate while really testing `mutant-nofffd`.
  The token matched, and only the failing directory showed it.
- **Skill cause:** Step 4 says to see the gate fail once, but nothing tells a plan that its red
  gate can be answered by another tree's build. Each plan writes its own red script (F2), and
  this pitfall is invisible from its output.
- **Recommended fix:** the promoted `red_on_base.sh` (F2) stamps every extracted file before
  building and requires a `Compiling <crate> v` line in its log. Its report names the directory
  the test failed under, so a reviewer can tell base from mutant.
- **Confidence:** high · **Effort:** small

## F4 — The red gate's `Compiling xdu v` guard cannot see a reused library
`origin=xdu-review:3 severity=medium category=tooling status=open target=.agents/factory/bin/ (the F2 helper)`
- **What happened:** to check F3's hardening, review removed only the `touch` line from a
  scratch copy of `verify/red.sh` and ran `mutant-nofffd` then `base` into one target dir. The
  `base` run reused the mutant's library: the `libxdu` rlib's mtime did not move, and the test
  failed under `…/bad�name`, the mutant's signature, not under `…/données`. The gate still
  printed `red: … fails on base`. Its log carried `Compiling xdu v0.6.0 (…/base.*/src)`, because
  the `tests/` overlay is copied with fresh mtimes, so the test targets always recompile and
  cargo prints the package's line whether or not the library did. With the `touch` in place the
  gate is sound: the rlib was restamped and `base` failed under `…/données`.
- **Skill cause:** F3's recommended fix names this guard as the backstop the promoted helper
  carries, so promoting it as written ships a check that cannot fail in the case it exists for.
  The script also accepts any directory holding `Cargo.toml` and `tests/` as the root, and
  `git archive` run from a subdirectory archives only that subdirectory.
- **Recommended fix:** in the F2 helper, keep the stamp and replace the guard with one that
  sees the library: `cargo clean -p <crate>` in the shared target dir before each tree, or
  require the library artifact's mtime to move. Take the root from
  `git rev-parse --show-toplevel`. Amend F3's recommended fix to match.
- **Confidence:** high · **Effort:** small

## F5 — A repo-wide search walks the blind reviewer into `spec/`
`origin=xdu-review:step2 severity=low category=steering status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** the blind reviewer ran a recursive search from the repository root, and its
  output carried a few lines of `PLAN.md` and `TECH.md`: file and test names from `verify:`
  commands, no rationale. It reported this itself, and no verdict depended on it.
- **Skill cause:** Step 2 strips `spec/` from the diff and forbids reading the plan, but says
  nothing about searches. On a committed branch, a bare `grep -r` or `rg` over the root reads
  `spec/` as readily as `src/`.
- **Recommended fix:** add to Step 2's conduct rule and the rubric's *Reviewer conduct*: search
  with `git grep … -- . ':(exclude)spec/'` or `--exclude-dir=spec`, never a bare recursive search
  of the root. This tightens blind-review integrity and weakens no gate.
- **Confidence:** high · **Effort:** small
