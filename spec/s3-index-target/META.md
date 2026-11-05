# META — Build an index into S3 and query it in place

> **Harness feedback log** for this feature — the producer artifact of the factory's self-improvement
> loop. Written by the lifecycle skills (`xdu-feature` / `xdu-plan` / `xdu-build` / `xdu-review`) when the
> **skillset itself** costs something; read by `xdu-publish` (surfaced in the PR) and applied by
> `/xdu-harness`. This file is **orthogonal** to the `GOAL → PLAN → TECH → REVIEW` spine — it is about
> the *toolchain*, not the feature — and is retained on merge like the rest of `spec/{slug}/`.
>
> **Silence is the default.** The bar for a finding is one test: *was this the **skill's** fault — not
> mine, not the task's?* A merely-hard task, a self-inflicted error, or a one-off content/code issue
> (that belongs in `GOAL.md` / `REVIEW.md`) is **not** a finding. The blind `xdu-review` correctness
> reviewer never reads this file — it would leak author intent.

- **slug:** s3-index-target

## Friction findings

Zero or more findings, appended below — each a markdown **section** so appending is a low-corruption
operation and a stdlib parser reads them (`uv run --with pyyaml python .agents/factory/bin/meta_status.py
spec/{slug}/META.md`). Skills always write `status=open`; only `/xdu-harness` flips it. `target` is a
best-guess file with **no line number** (re-derive the exact edit at apply time to avoid staleness). If
an equivalent finding already exists, append "· seen again" to its title instead of duplicating —
recurrence is signal, not bloat.

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

## F1 — A pilot split's deferred scope has no named destination or owner
`origin=xdu-feature:step4 severity=medium category=missing-guidance status=open target=.agents/skills/xdu-feature/SKILL.md`
- **What happened:** The size circuit-breaker split this seed into a pilot and follow-ups. The skill
  says to "record the deferred scope in Non-goals", while `methodology.md` says a deferral becomes an
  `issues/{slug}.md` plus a `ROADMAP.md` entry. Nothing says which step files those seeds, or when,
  so the GOAL had to guess that they are filed when the cycle publishes.
- **Skill cause:** The circuit-breaker and the deferral convention each say where scope goes, and
  neither names the step that turns a pilot's Non-goals into seeds.
- **Recommended fix:** In the circuit-breaker bullet, state that each deferred item in Non-goals
  names its future seed path, and that `xdu-publish` files the seeds and `ROADMAP.md` entries (or
  that `xdu-feature` files them itself, if the shaping step should own it).
- **Confidence:** med · **Effort:** small

## F2 — The "on `main`" pre-flight does not admit a cycle worktree detached at `origin/main`
`origin=xdu-feature:step1 severity=low category=instruction status=open target=.agents/skills/xdu-feature/SKILL.md`
- **What happened:** The cycle ran in a worktree detached at `origin/main`. Step 1 says to stop
  unless the branch is `main`, and `git switch -c {branch} main` names the local `main`, which can be
  behind `origin/main`. The dispatcher had to say "treat it as on main" for the step to proceed.
- **Skill cause:** The safety check is written for the single-checkout flow only, though the factory
  now runs cycles in worktrees.
- **Recommended fix:** Accept a detached `HEAD` equal to `origin/main` after the fetch as "on main",
  and branch from `HEAD` in that case rather than from the local `main`.
- **Confidence:** med · **Effort:** small
