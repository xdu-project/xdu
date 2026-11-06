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
