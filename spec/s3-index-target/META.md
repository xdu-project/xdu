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

## F3 — The adversarial-review rule for gate-shaped work reads as lean-path only
`origin=xdu-plan:step3 severity=medium category=instruction status=open target=.agents/skills/xdu-plan/SKILL.md`
- **What happened:** This big-appetite plan makes test infrastructure carry an R-ID (a byte meter
  for a cost bound, an opt-in test tier with a "nothing ignored" CI guard). Step 3's "gate-shaped
  deliverables" exception, which requires an adversarial reviewer, sits under the lean-path bullet
  and says the fan-out "is not skippable on the lean path", so it was unclear whether the big path
  owes the same review. The planner ran one anyway.
- **Skill cause:** The rule's placement and wording scope it to the path where research is skipped,
  while the failure it guards (a gate that fails green) does not depend on appetite.
- **Recommended fix:** Lift the gate-shaped rule out of the lean-path bullets into its own
  appetite-independent paragraph: any phase whose deliverable is a test, meter or CI guard that an
  R-ID rests on gets an adversarial reviewer before `PLAN.md` is committed.
- **Confidence:** med · **Effort:** small

## F4 — set_phase.py re-serializes the whole frontmatter on its first use
`origin=xdu-build:P1 severity=low category=tooling status=open target=.agents/factory/bin/set_phase.py`
- **What happened:** The first `set_phase.py` call of the build (P1 to `in_progress`, hill to
  `crest`) rewrote the plan's hand-written frontmatter from flow style into block style, re-quoted
  scalars and folded every `verify:` string across lines. A two-field state change became a
  100-line diff, and the only way to see that no gate changed was to load both versions with PyYAML
  and compare the `verify:` values (they were equal).
- **Skill cause:** The plan step writes the frontmatter by hand in one style and the build step's
  mandated script emits another, so the first build commit always carries a cosmetic rewrite of
  every phase's gate, which is exactly the text a reviewer must not have to re-read.
- **Recommended fix:** Have `set_phase.py` emit the same style the TECH template uses (flow lists,
  one-line single-quoted `verify:`), or have `xdu-plan` generate the frontmatter through the script
  so both steps share one serializer.
- **Confidence:** high · **Effort:** small

## F5 — An "is it absent" gate reads cargo's exit status, so any cargo failure passes it
`origin=xdu-build:P3 severity=high category=template status=open target=.agents/skills/xdu-plan/SKILL.md`
- **What happened:** P3's dependency-floor gate is `if cargo tree --locked -e normal -i $c
  >/dev/null 2>&1; then …exit 1; fi`, so any non-zero exit counts as "absent". Run with the
  registry unreachable (a sandbox denial of `static.crates.io` here), the same probe exited
  non-zero for a reason other than absence and printed "absent" for all three crates. It also
  checks the host target only: on macOS a dependency gated to Linux (native-tls pulls
  `openssl-sys` only off Apple targets) is invisible to it.
- **Skill cause:** The planning and build guidance says "see the gate fail once" but gives no rule
  for absence probes, where the failing direction is the green one; the plan wrote the probe in the
  shape that cannot fail closed. Weakness in a gate that guards a binding floor is high severity.
- **Recommended fix:** Absence probes must match the tool's own "not found" text (`did not match
  any packages`) and fail on any other non-zero exit, and a dependency probe names `--target all`
  (or each release target). Add that rule to the gate-writing guidance and retune P3's `verify:`
  through `set_phase.py --verify-file` at the foreman's word.
- **Confidence:** high · **Effort:** small
- **P3 run B:** on the foreman's ruling, P3's `verify:` was retuned this way (TECH, P3 gate
  amendment). The finding stays open for the gate-writing guidance itself.
